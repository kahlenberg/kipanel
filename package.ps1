# Builds kipanel.exe and creates two outputs in .\dist (Windows):
#   1) dist\com.mubin.kipanel\        -> copy into Documents\KiCad\9.0\plugins\ (manual install)
#   2) dist\kipanel-pcm-windows.zip   -> KiCad Plugin and Content Manager: "Install from File..."
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$dist = Join-Path $PSScriptRoot "dist"
$plug = Join-Path $dist "com.mubin.kipanel"
$pcm  = Join-Path $dist "pcm"
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $plug | Out-Null
Copy-Item target\release\kipanel.exe, pcm\icon_*_24.png, pcm\icon_*_48.png $plug
# plugin.json in the repo names the Unix binary; Windows needs the .exe
$json = (Get-Content pcm\plugin.json -Raw).Replace('"entrypoint": "kipanel"', '"entrypoint": "kipanel.exe"')
[IO.File]::WriteAllText((Join-Path $plug "plugin.json"), $json)

# PCM layout: metadata.json, plugins\..., resources\icon.png
New-Item -ItemType Directory -Force (Join-Path $pcm "plugins"), (Join-Path $pcm "resources") | Out-Null
Copy-Item "$plug\*" (Join-Path $pcm "plugins")
$meta = (Get-Content pcm\metadata.json -Raw).Replace('@PLATFORM@', 'windows')
[IO.File]::WriteAllText((Join-Path $pcm "metadata.json"), $meta)
Copy-Item pcm\icon_64.png (Join-Path $pcm "resources\icon.png")
$zip = Join-Path $dist "kipanel-pcm-windows.zip"
Compress-Archive -Path "$pcm\*" -DestinationPath $zip -Force
Remove-Item $pcm -Recurse -Force
Write-Host "Done:"
Write-Host "  manual : $plug"
Write-Host "  PCM zip: $zip"
