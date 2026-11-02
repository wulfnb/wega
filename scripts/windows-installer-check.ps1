param([string]$Installer = "dist\Wega-Windows-x64-Setup.exe", [string]$ExpectedVersion = "")
$ErrorActionPreference = 'Stop'
$registration = 'HKCU:\Software\Clients\StartMenuInternet\Wega'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wulfnb.Wega_is1'
if (Test-Path $registration) { throw 'Refusing to replace an existing Wega registration in this test' }
if (Test-Path $uninstallKey) { throw 'Refusing to replace an existing Wega installation in this test' }
$destination = Join-Path $env:TEMP ('Wega-install-check-' + [guid]::NewGuid().ToString('N'))
$defaultDirectory = Join-Path $env:LOCALAPPDATA 'Programs\Wega'
$defaultExisted = Test-Path $defaultDirectory

function Invoke-Installer([string[]]$Arguments) {
    $process = Start-Process -FilePath (Resolve-Path $Installer) -ArgumentList (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') + $Arguments) -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
}

function Assert-Installation {
    foreach ($file in 'wega.exe', 'wega-engine.exe', 'WEGA-ENGINE-THIRD-PARTY-NOTICES.html', 'libEGL.dll', 'libGLESv2.dll', 'LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES.html', 'unins000.exe') {
        if (!(Test-Path (Join-Path $destination $file))) { throw "Installed file missing: $file" }
    }
    $entry = Get-ItemProperty $uninstallKey
    if ($entry.InstallLocation.TrimEnd('\') -ne $destination) { throw "Unexpected install location: $($entry.InstallLocation)" }
    if ($ExpectedVersion -and $entry.DisplayVersion -ne $ExpectedVersion) { throw "Installed version $($entry.DisplayVersion), expected $ExpectedVersion" }
}

try {
    Invoke-Installer @("/DIR=`"$destination`"")
    Assert-Installation
    $capabilities = Get-ItemProperty "$registration\Capabilities\URLAssociations"
    if ($capabilities.http -ne 'WegaURL' -or $capabilities.https -ne 'WegaURL') { throw 'URL capabilities are missing' }
    $command = (Get-Item 'HKCU:\Software\Classes\WegaURL\shell\open\command').GetValue('')
    if ($command -ne "`"$destination\wega.exe`" -- `"%1`"") { throw "Unexpected URL launch command: $command" }
    $startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Wega\Wega.lnk'
    if (!(Test-Path $startMenu)) { throw 'Start menu shortcut is missing' }

    # Upgrade on top: no /DIR, so the installer must find the previous directory
    # itself, and must remove binaries the new release no longer ships.
    $stale = Join-Path $destination 'wega-stale-plugin.dll'
    Set-Content -Path $stale -Value 'left over from an older release'
    Invoke-Installer @()
    Assert-Installation
    if (Test-Path $stale) { throw 'Upgrade left a stale DLL from the previous version' }
    if (Test-Path (Join-Path $destination 'unins001.exe')) { throw 'Upgrade registered a second uninstaller' }
    if (!$defaultExisted -and (Test-Path $defaultDirectory)) { throw 'Upgrade installed into the default directory instead of the existing one' }
} finally {
    $uninstaller = Join-Path $destination 'unins000.exe'
    if (Test-Path $uninstaller) {
        $process = Start-Process $uninstaller -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Uninstall failed: $($process.ExitCode)" }
    }
}
if (Test-Path $registration) { throw 'Uninstall left browser registration behind' }
if (Test-Path 'HKCU:\Software\Classes\WegaURL') { throw 'Uninstall left the URL handler behind' }
# The uninstaller hands off to a temporary copy, so its entry can outlive the waited process briefly.
for ($attempt = 0; (Test-Path $uninstallKey) -and $attempt -lt 30; $attempt++) { Start-Sleep -Seconds 1 }
if (Test-Path $uninstallKey) { throw 'Uninstall left the uninstall entry behind' }
Write-Output 'PASS: per-user installation, in-place upgrade, packaged files, browser registration, shortcut and uninstall'
