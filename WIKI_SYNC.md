# Gitea 공유 Wiki

프로젝트 선택 화면에서 **기존 프로젝트 참여하기**로 내부 Gitea 저장소에 참여할 수 있다. 이미 열어 둔 프로젝트는 **설정 → 위키 동기화**에서 연결한다. 공유 단위는 프로젝트의 `wiki/` 디렉터리다. 각 사용자는 독립적인 프로젝트와 작업 사본을 갖는다.

## 관리자 준비

1. 클라이언트에 시스템 Git과 OpenSSH 또는 OS 보안 자격 증명 도우미를 설치한다. 공용 `wiki-sync` 계정에 대상 저장소의 읽기·쓰기 권한을 부여한다.
2. 앱을 실행하는 OS 계정에 SSH 키와 검증한 서버 호스트 키를 배포하거나 HTTPS용 자격 증명을 OS 보안 저장소에 등록한다. Gitea의 SSH URL에 쓰이는 `git`은 전송용 로그인 이름이다. Gitea는 공개 키에 연결된 계정으로 권한을 확인한다.
3. 아래 환경 변수를 데스크톱 앱이 시작될 때 읽을 수 있게 설정한다. 신규 공유 Wiki는 Gitea에서 README·라이선스 없이 빈 저장소를 먼저 만든다.

| 변수 | 용도 | 예시 |
| --- | --- | --- |
| `LLM_WIKI_GITEA_ORIGIN` | 허용할 서버의 프로토콜, 호스트, 포트 | `ssh://gitea.internal:2222` |
| `LLM_WIKI_GITEA_REPOSITORY` | 연결 화면에 미리 채울 저장소 URL | `ssh://git@gitea.internal:2222/team/wiki.git` |
| `LLM_WIKI_EMBEDDING_ORIGIN` | DNS 이름으로 운영하는 내부 임베딩 서버의 허용 주소 | `http://models.internal:8080` |

HTTPS를 쓰면 첫 두 값 모두 `https://`로 설정한다. URL에는 토큰이나 비밀번호를 넣지 않는다. 자동 Git 작업은 인증 프롬프트와 HTTP 리디렉션을 끄고 SSH 호스트 키 검증을 요구한다. 임베딩은 기존 앱의 모델 설정을 재사용한다. 공유 페이지의 자동 색인은 루프백, 사설 IPv4, 또는 관리자가 지정한 내부 모델 주소에서 실행한다.

## 시작과 연결

- **프로젝트 전환 → 기존 프로젝트 참여하기**에서 사용자 ID와 기존 공유 저장소의 전체 SSH 또는 HTTPS URL을 입력한다. 상위 폴더를 선택하면 저장소 이름의 하위 폴더에 프로젝트가 열리고 자동 동기화가 시작된다. 같은 이름의 폴더가 있으면 다른 위치를 선택한다. 이전에 내려받은 사본은 **프로젝트 열기**로 다시 연다.
- 사용자 ID는 이 장치의 위키 변경 기록에 쓰는 작성자 이름이다. 예를 들어 `홍길동`, `연구팀_01`을 사용할 수 있다. 한글·영문·숫자로 시작하는 1~64자를 허용하며 두 번째 글자부터 점, 밑줄, 하이픈을 쓸 수 있다. 참여에 성공하거나 설정에서 저장하면 `app-state.json`에 기억되며 다음 커밋에 `ID <ID@local.invalid>`로 기록된다. 로컬 프로젝트 생성과 열기는 ID 입력 없이 사용할 수 있다.
- **Clone existing repository**는 임시 디렉터리에 원격 기본 브랜치를 가져와 파일 유형을 검사한 후 적용한다. 기본 템플릿을 포함해 로컬 Wiki에 파일이 있으면 **Back up local Wiki and clone**을 별도로 선택한다. 백업은 프로젝트의 `.llm-wiki/wiki-backup-<UUID>/`에 보관하며 설정에 경로를 표시한다. 교체 도중 앱이 중단되어 Wiki 디렉터리가 비면 다음 프로젝트 열기에서 이전 디렉터리를 복원한다.
- **Connect local Wiki**는 빈 원격 저장소에 기존 Markdown을 게시한다. 양쪽에 이력이 있는 경우에는 기존 자료를 백업한 뒤 Clone하거나 새 프로젝트에서 Clone한다. 사용자가 별도로 관리하던 Git 이력은 자동으로 업로드하지 않는다.
- **Auto sync**를 해제하면 수동 동기화를 사용할 수 있다. **Disconnect**는 연결을 해제하고 로컬 파일, Git 이력과 백업을 보존한다. 같은 URL에 다시 연결할 수 있다.

## 동기화와 복구

1. 현재 프로젝트의 Wiki Markdown 변경을 3초 동안 모은다. 원본 감시기가 켜져 있으면 해당 감시기의 이벤트를 사용하고, 꺼져 있으면 Wiki 전용 감시기를 사용한다. 로컬 변경이 없어도 30초마다 원격 브랜치를 확인한다.
2. 인제스트와 같은 프로젝트 쓰기 잠금을 획득한 후 Rust 작업 스레드가 Git을 실행한다. 허용된 일반 Markdown의 추가·수정·삭제만 명시적으로 스테이징한다. 숨김 경로, 미디어, 원본, 로컬 설정 및 DB는 공유 대상에서 제외한다. 원격 브랜치에 허용 범위 밖의 파일이나 심볼릭 링크가 있으면 적용을 중단한다.
3. 로컬 변경을 먼저 커밋하고 원격 변경을 fetch·merge한 뒤 로컬 브랜치가 앞서면 push한다. 푸시 경합에는 한 번 다시 시도한 뒤 다음 주기로 넘긴다. 네트워크 장애 시 커밋이 로컬에 남는다. 인증 오류는 관리자가 자격 증명 또는 서버 신뢰 설정을 확인하고 **Sync now**로 다시 시도한다.
4. Clone·Merge가 바꾼 경로는 Wiki 저장소 안에 로컬 기록으로 남긴다. 파일 트리, 화면과 데이터 버전을 갱신한다. 삭제한 페이지의 벡터를 정리하고, 임베딩이 활성화되어 있으면 변경 페이지를 기존 인덱서로 처리한다. 색인 실패는 별도로 표시하고 이후 재시도한다. 일반 검색은 로컬 Markdown을 계속 읽는다.
5. **Conflict**에서는 자동 Push가 멈춘다. **Open Wiki folder**로 충돌 파일을 열어 내용을 수정한 뒤 **Retry after resolution**을 누른다. Git 충돌 표시가 남은 파일은 재개를 막는다. 사용자가 선택한 해결본으로 병합 커밋을 만들며 양쪽의 이전 커밋을 유지한다.

상태 화면은 미연결, 연결 중, 연결됨, 동기화 중, 동기화 완료, 오프라인, 인증 오류, 충돌, 일반 오류와 마지막 성공 시각을 표시한다. 자동 동기화는 현재 열린 프로젝트에서 동작한다. 앱의 Markdown 편집 모드 동안에는 Git 작업을 유예한다. 외부 편집기는 자체적으로 저장 시점을 관리한다.

## 구현 위치

| 파일 | 역할 |
| --- | --- |
| `src-tauri/wiki-sync/Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | 독립적으로 테스트할 수 있는 Git 코어 크레이트와 데스크톱 의존성 |
| `src-tauri/wiki-sync/src/lib.rs` | 주소·경로 검증, Git CLI, 연결과 백업, 병합, 푸시, 충돌 및 변경 기록 |
| `src-tauri/wiki-sync/src/tests.rs` | 로컬 bare 원격 저장소를 사용하는 통합 테스트와 유효성 검사 |
| `src-tauri/src/commands/git_sync.rs` | 프로젝트별 직렬 실행, Tauri 명령, 전용 감시 및 원격 페이지 임베딩 |
| `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` | 명령 및 감시기 상태 등록 |
| `src-tauri/src/commands/file_sync.rs` | Markdown 이벤트, Git 메타데이터 제외, 원격 쓰기 스냅샷 갱신 |
| `src-tauri/src/commands/project.rs` | 프로젝트 열기 시 중단된 Clone 복구 |
| `src-tauri/src/commands/page_embedding.rs`, `src-tauri/src/commands/search.rs` | 기존 검색·임베딩 재사용, Git 메타데이터 제외, 내부 모델 전송 제한 |
| `src/lib/embedding.ts`, `src-tauri/src/commands/project_maintenance.rs` | 삭제된 공유 페이지의 벡터 정리, 수동 인덱스 재구축 시 Git 메타데이터 제외 |
| `src/lib/wiki-sync.ts`, `src/lib/wiki-sync.test.ts` | 타이머, 수명주기, UI·검색 갱신, 사용자 ID와 타이밍 회귀 테스트 |
| `src/lib/project-store.ts`, `src/main.tsx` | 장치 사용자 ID 저장 및 시작 시 불러오기 |
| `src/App.tsx`, `src/components/settings/settings-view.tsx`, `src/components/settings/sections/wiki-sync-section.tsx` | 프로젝트 전환, 설정, 상태 알림과 연결·충돌 조작 |
| `src/components/editor/wiki-editor.tsx`, `src/components/layout/preview-panel.tsx`, `src/lib/project-file-sync.ts` | 편집 보호, 저장 잠금, 선택 파일 갱신 |
| `src/components/editor/wiki-reader.tsx`, `src/components/editor/frontmatter-panel.tsx`, `src/lib/wiki-page-resolver.ts` | 누락 이미지 표시 및 공유 페이지의 원본 경로 확인 |
| `src/lib/raw-source-resolver.ts`, `src/components/search/search-view.tsx`, `src/components/chat/chat-message.tsx` | 공유 이미지와 장치의 로컬 원본 연결 확인 |

### Tauri 명령과 이벤트

| 명령 | 동작 |
| --- | --- |
| `git_sync_managed` | 내부 서버 배포 여부 확인 및 자동 GitHub 업데이트 조회 생략 |
| `git_sync_status` | 관리자 기본값, 연결, 충돌 파일, 마지막 성공 시각, 미처리 변경 조회 |
| `git_sync_connect` | Clone, 백업 후 Clone, 로컬 Wiki 연결 |
| `git_sync_now` | 수동·자동 동기화 또는 사용자 요청에 따른 충돌 해결 완료 |
| `git_sync_enable`, `git_sync_disconnect` | 자동 실행 설정 및 연결 해제 |
| `git_sync_watch`, `git_sync_unwatch` | 원본 감시가 꺼진 프로젝트의 변경 알림 등록·해제 |
| `git_sync_embed_page` | 승인된 내부 모델을 통한 변경 페이지 색인 |
| `git_sync_acknowledge` | 반영이 끝난 변경 기록의 토큰 확인 및 정리 |

원본 감시기는 `wiki-sync://dirty`에 프로젝트 ID를 보내고 전용 감시기는 Tauri 채널을 사용한다. 원격 쓰기는 기존 파일 감시기의 앱 쓰기로 표시한다. 편집 도중 추가된 변경은 후속 작업 또는 다음 폴링에서 확인한다.

## 검증과 운영 범위

- Rust 코어 테스트는 `cd src-tauri && cargo test --locked -p llm-wiki-sync`로 실행한다. 의존성이 준비된 환경에서는 `--offline`을 추가할 수 있다. 테스트용 원격은 임시 로컬 bare 저장소다.
- 프런트엔드 빌드는 `npm run build`, 외부 모델 호출을 제외한 회귀 테스트는 `npm run test:mocks`로 실행한다.
- 바이너리와 Git 하위 모듈은 지원하지 않는다. 기존 원본 파일과 미디어가 없는 장치에서도 공유 페이지 본문을 읽을 수 있다. 원본 내용이 동일한지 서버에서 증명하는 기능은 없다.
- Gitea의 푸시 감사 기록에는 공용 계정이 표시된다. 사용자 ID는 장치에서 입력한 커밋 작성자이며 개인별 서버 인증이나 서명을 제공하지 않는다.
- 연결할 Git 서버, 호스트 키, 자격 증명, 내부 모델 및 망 정책은 설치 환경에서 배포한다. 내부 서버를 지정하면 시작 시 GitHub 업데이트 조회를 건너뛴다. 사용자가 직접 실행하는 LLM·웹 검색 등은 각각의 기존 설정을 따른다.

### 이번 변경의 검증 결과

| 검사 | 결과 |
| --- | --- |
| `npm run build:desktop` | MCP 서버, TypeScript, Vite 프로덕션 번들 빌드 통과 |
| `npm run test:mocks` | 140개 파일에서 2,072개 테스트 통과 |
| `cargo test --locked --offline -p llm-wiki-sync` | Git 코어 테스트 11개 통과 |
| `cargo check --locked --tests` | Linux Tauri 백엔드와 테스트 대상 컴파일 검사 통과 |

실제 사내 Gitea의 SSH 키·HTTPS 자격 증명, 운영 임베딩 모델, OS별 설치 패키지 실행은 이번 검증 범위에 포함되지 않았다. Git 통신은 로컬 bare 원격을 이용해 검증했고 프런트엔드 연동 테스트는 Tauri 명령을 모의 실행했다.
