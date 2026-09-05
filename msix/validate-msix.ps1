[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Package,
    [Parameter(Mandatory)] [ValidatePattern('^\d+\.\d+\.\d+$')] [string]$Version,
    [string]$WorkingDirectory
)

$ErrorActionPreference = 'Stop'

$VersionParts = $Version.Split('.') | ForEach-Object { [int]$_ }

if (-not $WorkingDirectory) {
    $WorkingDirectory = Join-Path $PSScriptRoot '..\release\validation'
}

function Find-WindowsSdkTool([string]$Name) {
    $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $tool = Get-ChildItem -Path $sdkBin -Directory | Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\$Name" } |
        Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $tool) { throw "$Name was not found in the Windows SDK." }
    $tool
}

$packagePath = [System.IO.Path]::GetFullPath($Package)
if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) { throw "MSIX package was not found: $packagePath" }
Remove-Item -LiteralPath $WorkingDirectory -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $WorkingDirectory -Force | Out-Null

& (Find-WindowsSdkTool 'makeappx.exe') unpack /p $packagePath /d $WorkingDirectory /o
if ($LASTEXITCODE -ne 0) { throw "makeappx unpack failed with exit code $LASTEXITCODE." }

$manifest = [xml](Get-Content -LiteralPath (Join-Path $WorkingDirectory 'AppxManifest.xml') -Raw)
if ($manifest.Package.Identity.Version -ne "$Version.0") { throw "Unexpected package version: $($manifest.Package.Identity.Version)" }
foreach ($binary in @('LastKey.exe', 'LastKey.Settings.exe')) {
    $binaryVersion = (Get-Item -LiteralPath (Join-Path $WorkingDirectory $binary)).VersionInfo
    if ($binaryVersion.FileMajorPart -ne $VersionParts[0] -or
        $binaryVersion.FileMinorPart -ne $VersionParts[1] -or
        $binaryVersion.FileBuildPart -ne $VersionParts[2] -or
        $binaryVersion.FilePrivatePart -ne 0) {
        throw "Executable version $($binaryVersion.FileVersion) in $binary does not match package version $Version."
    }
}
foreach ($path in @('LastKey.exe', 'LastKey.Settings.exe', 'resources.pri', 'Assets\Square44x44Logo.png', 'Assets\Square150x150Logo.png', 'LICENSE.txt', 'LICENSES\MIT.txt', 'NOTICE.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $WorkingDirectory $path))) { throw "Required package file is missing: $path" }
}
Write-Output "Validated $packagePath"
