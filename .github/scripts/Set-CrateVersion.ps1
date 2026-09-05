[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [ValidatePattern('^\d+\.\d+\.\d+$')]
  [string]$Version,

  [string]$CargoTomlPath = 'Cargo.toml',

  [string]$CargoLockPath = 'Cargo.lock'
)

# Sets the package version in Cargo.toml (first top-level `version` line) and
# the matching lastkey entry in Cargo.lock, so --locked builds keep passing
# without running cargo. Both files must already contain the expected shapes.
$tomlPattern = [regex]'(?m)^version\s*=\s*"\d+\.\d+\.\d+"'
$toml = [IO.File]::ReadAllText($CargoTomlPath)
if (-not $tomlPattern.IsMatch($toml)) {
  throw "No package version line found in $CargoTomlPath."
}
$updatedToml = $tomlPattern.Replace($toml, 'version = "' + $Version + '"', 1)
[IO.File]::WriteAllText($CargoTomlPath, $updatedToml, [Text.UTF8Encoding]::new($false))

$lockPattern = [regex]'(?s)(name = "lastkey"\r?\nversion = ")[^"]+(")'
$lock = [IO.File]::ReadAllText($CargoLockPath)
if (-not $lockPattern.IsMatch($lock)) {
  throw 'No lastkey package entry found in Cargo.lock.'
}
$updatedLock = $lockPattern.Replace($lock, '${1}' + $Version + '$2', 1)
[IO.File]::WriteAllText($CargoLockPath, $updatedLock, [Text.UTF8Encoding]::new($false))
