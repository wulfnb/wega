# Pinned upstream MSVC SDK. Its runtime DLL closure is bundled by bundle-windows-media.py.
$ErrorActionPreference = 'Stop'
$version = '1.28.8'
$installer = Join-Path $env:RUNNER_TEMP "gstreamer-msvc-$version.exe"
$sdk = Join-Path $env:RUNNER_TEMP 'wega-gstreamer'
$url = "https://gstreamer.freedesktop.org/pkg/windows/$version/msvc/gstreamer-1.0-msvc-x86_64-$version.exe"
$expected = 'E4C41EE6EE9C6C32A45D198463EC08D79BAC3BD5FA58ADC93D13FE4A08ABC9D9'
# CI restores the installer from actions/cache; download only when it is absent or stale.
if (!(Test-Path $installer) -or (Get-FileHash $installer -Algorithm SHA256).Hash -ne $expected) {
    Invoke-WebRequest $url -OutFile $installer
}
if ((Get-FileHash $installer -Algorithm SHA256).Hash -ne $expected) { throw 'GStreamer SDK checksum mismatch' }
$process = Start-Process $installer -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/portable=1', '/CURRENTUSER', '/TYPE=devel', "/DIR=`"$sdk`"" -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "GStreamer SDK installation failed: $($process.ExitCode)" }
if (!(Test-Path "$sdk\bin\gst-inspect-1.0.exe")) { throw 'GStreamer SDK has an unexpected layout' }
"GSTREAMER_1_0_ROOT_MSVC_X86_64=$sdk" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
"PKG_CONFIG_PATH=$sdk\lib\pkgconfig" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
"$sdk\bin" | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
$actualVersion = & "$sdk\bin\gst-inspect-1.0.exe" --version
if ($LASTEXITCODE -ne 0 -or ($actualVersion -join ' ') -notmatch 'GStreamer 1\.28\.8(?:\s|$)') { throw 'Installed SDK version does not match the pinned release' }
@{ version = $version; installer_sha256 = $expected; source = $url } | ConvertTo-Json | Set-Content "$sdk\WEGA-SDK-PROVENANCE.json" -Encoding utf8

# Portable SDK installs do not run Microsoft's machine-wide redistributable.
# Bundle release CRT DLLs from Visual Studio's dedicated REDIST directory instead.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$visualStudio = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or !$visualStudio) { throw 'Visual Studio C++ redistributable directory could not be located' }
$runtime = Get-ChildItem "$visualStudio\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName 'vcruntime140.dll') } |
    Sort-Object { (Get-Item (Join-Path $_.FullName 'vcruntime140.dll')).VersionInfo.FileVersionRaw } -Descending |
    Select-Object -First 1
if (!$runtime) { throw 'Release x64 Visual C++ CRT redistributable files are missing' }
"WEGA_VC_RUNTIME_DIR=$($runtime.FullName)" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
