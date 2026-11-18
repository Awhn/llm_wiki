use std::path::Path;

pub(super) async fn extract(
    resource_dir: Option<&Path>,
    document: &Path,
) -> Result<String, String> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let dir = resource_dir.ok_or("rhwp 리소스 경로를 찾을 수 없습니다.")?;
        parser::run(&dir.join("rhwp/rhwp.exe"), document).await
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (resource_dir, document);
        Err("한글 문서 파싱은 Windows x64에서 지원합니다.".into())
    }
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
mod parser {
    use serde::Deserialize;
    use std::{io, path::Path, process::Stdio, time::Duration};
    use tokio::{
        io::{AsyncRead, AsyncReadExt},
        process::Command,
    };

    #[derive(Deserialize)]
    struct Export {
        chunks: Vec<Chunk>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Chunk {
        heading_path: Vec<String>,
        text: String,
    }

    fn markdown(stdout: &[u8]) -> Result<String, String> {
        let document: Export = serde_json::from_slice(stdout)
            .map_err(|e| format!("rhwp 결과를 읽지 못했습니다 ({e})."))?;
        let mut previous = Vec::new();
        let mut blocks = Vec::new();
        for chunk in document.chunks {
            let common = chunk
                .heading_path
                .iter()
                .zip(&previous)
                .take_while(|(a, b)| a == b)
                .count();
            for (depth, heading) in chunk.heading_path.iter().enumerate().skip(common) {
                if !heading.trim().is_empty() {
                    blocks.push(format!(
                        "{} {}",
                        "#".repeat((depth + 1).min(6)),
                        heading.trim()
                    ));
                }
            }
            previous = chunk.heading_path;
            if !chunk.text.trim().is_empty() {
                blocks.push(chunk.text);
            }
        }
        if blocks.is_empty() {
            return Err("한글 문서에서 추출할 수 있는 텍스트가 없습니다.".into());
        }
        Ok(blocks.join("\n\n"))
    }

    async fn read_limited(pipe: impl AsyncRead + Unpin, limit: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        pipe.take(limit + 1).read_to_end(&mut bytes).await?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rhwp 출력 크기 제한을 초과했습니다.",
            ));
        }
        Ok(bytes)
    }

    pub(super) async fn run(executable: &Path, document: &Path) -> Result<String, String> {
        let document = std::path::absolute(document).map_err(|e| e.to_string())?;
        let mut command = Command::new(executable);
        command
            .arg("export-llm")
            .arg(document)
            .args(["--format", "json", "--max-tokens", "8192"])
            .current_dir(executable.parent().ok_or("rhwp 실행 경로 오류")?)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = command
            .spawn()
            .map_err(|e| format!("포함된 rhwp 파서를 실행할 수 없습니다 ({e})."))?;
        let stdout = child.stdout.take().ok_or("rhwp 출력 연결 오류")?;
        let stderr = child.stderr.take().ok_or("rhwp 오류 출력 연결 오류")?;
        let output = async {
            tokio::try_join!(
                child.wait(),
                read_limited(stdout, 64 * 1024 * 1024),
                read_limited(stderr, 64 * 1024)
            )
        };
        let (status, stdout, stderr) = tokio::time::timeout(Duration::from_secs(120), output)
            .await
            .map_err(|_| "rhwp 문서 처리 시간(120초)을 초과했습니다.")?
            .map_err(|e| format!("rhwp 출력 처리 실패 ({e})."))?;
        if !status.success() {
            let detail: String = String::from_utf8_lossy(&stderr)
                .trim()
                .chars()
                .take(500)
                .collect();
            return Err(format!("한글 문서 파싱 실패 ({status}) {detail}"));
        }
        markdown(&stdout)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reassembles_headings_across_chunks() {
            let result = markdown(br##"{"chunks":[{"headingPath":["A"],"text":"one"},{"headingPath":["A","B"],"text":"two"},{"headingPath":["A","B"],"text":"three"},{"headingPath":["C"],"text":"four"}]}"##).unwrap();
            assert_eq!(result, "# A\n\none\n\n## B\n\ntwo\n\nthree\n\n# C\n\nfour");
            assert!(markdown(br#"{"chunks":[]}"#).is_err());
            assert!(markdown(b"invalid").is_err());
        }

        #[tokio::test]
        async fn missing_parser_is_an_error() {
            let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("rhwp/no-such-parser.exe");
            assert!(run(&missing, Path::new("document.hwp")).await.is_err());
        }

        #[cfg(all(windows, target_arch = "x86_64"))]
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn bundled_parser_reads_both_hangul_formats() {
            use crate::commands::fs::{
                preprocess_file, read_file, read_preprocessed_cache, set_resource_dir_hint,
            };

            let exe = std::env::current_exe().unwrap();
            let resources = exe
                .ancestors()
                .find(|dir| dir.join("rhwp/rhwp.exe").is_file())
                .expect("Tauri must copy rhwp into the Windows build's resource directory");
            set_resource_dir_hint(resources.to_owned());
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("rhwp/fixtures");
            let temp = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir(&temp).unwrap();
            for name in ["회의 기록.hwp", "회의 기록.hwpx"] {
                let source = temp.join(name);
                std::fs::copy(fixtures.join(name), &source).unwrap();
                let path = source.to_str().unwrap().to_owned();
                let preview = read_file(path.clone(), None).await.unwrap();
                assert!(preview.contains("# 회의 결과"));
                assert!(preview.contains("검토를 마쳤습니다."));
                assert!(preview.contains("| 의자 | 12 |"));
                assert_eq!(preprocess_file(path.clone()).await.unwrap(), preview);
                assert_eq!(
                    read_preprocessed_cache(&source).as_deref(),
                    Some(preview.as_str())
                );
                assert_eq!(read_file(path, None).await.unwrap(), preview);
            }
            let corrupt = temp.join("손상.HWP");
            std::fs::write(&corrupt, b"invalid document").unwrap();
            assert!(preprocess_file(corrupt.to_str().unwrap().into())
                .await
                .is_err());
            assert!(read_preprocessed_cache(&corrupt).is_none());
            std::fs::remove_dir_all(temp).unwrap();
        }
    }
}
