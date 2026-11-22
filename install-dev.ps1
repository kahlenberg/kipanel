# Build and copy straight into the KiCad plugin folder (Windows).
#   .\install-dev.ps1                      # KiCad 9.0
#   .\install-dev.ps1 -KiCadVersion 10.0   # other KiCad version
param([string]$KiCadVersion = "9.0")
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$dst = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "KiCad\$KiCadVersion\plugins\com.mubin.kipanel"
New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item target\release\kipanel.exe, pcm\icon_*_24.png, pcm\icon_*_48.png $dst -Force
# plugin.json in the repo names the Unix binary; Windows needs the .exe
$json = (Get-Content pcm\plugin.json -Raw).Replace('"entrypoint": "kipanel"', '"entrypoint": "kipanel.exe"')
[IO.File]::WriteAllText((Join-Path $dst "plugin.json"), $json)
Write-Host "Installed to $dst"
Write-Host "  -> in KiCad (PCB editor): Tools > External Plugins > Refresh Plugins"
