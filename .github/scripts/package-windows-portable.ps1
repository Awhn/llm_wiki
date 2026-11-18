param(
  [Parameter(Mandatory = $true)]
  [string]$Version
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "../..")
$ExePath = Join-Path $RepoRoot "src-tauri/target/release/llm-wiki.exe"
$PdfiumPath = Join-Path $RepoRoot "src-tauri/pdfium/pdfium.dll"
$RhwpRoot = Join-Path $RepoRoot "src-tauri/rhwp/windows-x64"
$McpRoot = Join-Path $RepoRoot "mcp-server"
$DistRoot = Join-Path $RepoRoot "dist-portable"
$PortableRoot = Join-Path $DistRoot "LLM-Wiki-$Version-windows-x64-portable"
$ZipPath = Join-Path $DistRoot "LLM-Wiki-$Version-windows-x64-portable.zip"

if (!(Test-Path $ExePath)) {
  throw "Tauri executable was not found at $ExePath"
}
if (!(Test-Path $PdfiumPath)) {
  throw "PDFium DLL was not found at $PdfiumPath"
}
foreach ($Path in @(
  (Join-Path $McpRoot "package.json"),
  (Join-Path $McpRoot "dist"),
  (Join-Path $McpRoot "node_modules")
)) {
  if (!(Test-Path $Path)) {
    throw "Required MCP resource was not found at $Path. Run npm --prefix mcp-server ci and npm run mcp:build first."
  }
}

if (Test-Path $PortableRoot) {
  Remove-Item -Recurse -Force $PortableRoot
}
if (Test-Path $ZipPath) {
  Remove-Item -Force $ZipPath
}
New-Item -ItemType Directory -Force $PortableRoot | Out-Null

Copy-Item $ExePath (Join-Path $PortableRoot "LLM Wiki.exe")

Copy-Item -Recurse $RhwpRoot (Join-Path $PortableRoot "rhwp")
Copy-Item (Join-Path $RepoRoot "LICENSE") (Join-Path $PortableRoot "LICENSE.txt")

New-Item -ItemType Directory -Force (Join-Path $PortableRoot "pdfium") | Out-Null
Copy-Item $PdfiumPath (Join-Path $PortableRoot "pdfium/pdfium.dll")

$PortableMcpRoot = Join-Path $PortableRoot "mcp-server"
New-Item -ItemType Directory -Force $PortableMcpRoot | Out-Null
Copy-Item (Join-Path $McpRoot "package.json") (Join-Path $PortableMcpRoot "package.json")
Copy-Item -Recurse (Join-Path $McpRoot "dist") (Join-Path $PortableMcpRoot "dist")
Copy-Item -Recurse (Join-Path $McpRoot "node_modules") (Join-Path $PortableMcpRoot "node_modules")

@"
LLM Wiki Windows Portable

ZIP 전체를 폴더에 푼 뒤 "LLM Wiki.exe"를 실행하세요.
rhwp, pdfium, mcp-server 폴더는 실행 파일 옆에 두세요.
한글 문서 파서는 포함되어 있습니다. 앱 실행에는 Microsoft Edge WebView2 런타임이 필요합니다.
설정과 대화 기록 등 앱 데이터는 Windows 사용자 프로필에 저장됩니다.
Gitea 동기화 기능에는 Git for Windows가 필요합니다.

Extract the entire ZIP and run "LLM Wiki.exe". Keep rhwp/, pdfium/, and mcp-server/ beside it.
Requires Microsoft Edge WebView2 Runtime. Gitea sync also requires Git for Windows.

This portable package does not install start-menu shortcuts or auto-update hooks. It still stores app data in the normal LLM Wiki application data directory.
"@ | Set-Content -Encoding UTF8 (Join-Path $PortableRoot "README-portable.txt")

[System.IO.Compression.ZipFile]::CreateFromDirectory($PortableRoot, $ZipPath)
Write-Host "Created $ZipPath"
