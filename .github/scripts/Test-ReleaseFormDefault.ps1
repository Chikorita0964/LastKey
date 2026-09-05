[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$newline = [Environment]::NewLine

function Assert-Equal {
  param(
    [Parameter(Mandatory)]
    [string]$Expected,

    [Parameter(Mandatory)]
    [string]$Actual,

    [Parameter(Mandatory)]
    [string]$Message
  )

  if ($Actual -ne $Expected) {
    throw "$Message Expected: '$Expected'. Actual: '$Actual'."
  }
}

function Assert-True {
  param(
    [Parameter(Mandatory)]
    [bool]$Condition,

    [Parameter(Mandatory)]
    [string]$Message
  )

  if (-not $Condition) {
    throw $Message
  }
}

function New-ReleaseWorkflowFixture {
  param(
    [Parameter(Mandatory)]
    [AllowEmptyString()]
    [string]$OpeningQuote,

    [Parameter(Mandatory)]
    [AllowEmptyString()]
    [string]$ClosingQuote
  )

  @(
    'name: Release'
    ''
    'on:'
    '  workflow_dispatch:'
    '    inputs:'
    '      tag:'
    "        description: $OpeningQuote" + 'Current version: v1.0.1. Next release tag to create.' + $ClosingQuote
    '        required: true'
    '        default: v1.0.2'
    '        type: string'
  ) -join $newline + $newline
}

$repositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$updateScriptPath = Join-Path $PSScriptRoot 'Update-ReleaseFormDefault.ps1'
$testDirectory = Join-Path $repositoryRoot ("out\release-form-tests-{0}" -f [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($testDirectory) | Out-Null

try {
  $successCases = @(
    @{ Name = 'unquoted'; OpeningQuote = ''; ClosingQuote = '' }
    @{ Name = 'double-quoted'; OpeningQuote = '"'; ClosingQuote = '"' }
    @{ Name = 'single-quoted'; OpeningQuote = "'"; ClosingQuote = "'" }
  )

  foreach ($case in $successCases) {
    $workflowPath = Join-Path $testDirectory "$($case.Name).yml"
    $fixture = New-ReleaseWorkflowFixture -OpeningQuote $case.OpeningQuote -ClosingQuote $case.ClosingQuote
    [IO.File]::WriteAllText($workflowPath, $fixture, [Text.UTF8Encoding]::new($false))

    $nextReleaseTag = & $updateScriptPath -ReleaseTag 'v1.2.3' -WorkflowPath $workflowPath
    Assert-Equal -Expected 'v1.2.4' -Actual $nextReleaseTag -Message "$($case.Name) next version is incorrect."

    $updated = [IO.File]::ReadAllText($workflowPath)
    $expectedDescription = "        description: $($case.OpeningQuote)Current version: v1.2.3. Next release tag to create.$($case.ClosingQuote)"
    Assert-True -Condition $updated.Contains($expectedDescription) -Message "$($case.Name) description was not updated."
    Assert-True -Condition $updated.Contains('        default: v1.2.4') -Message "$($case.Name) default was not updated."
  }

  $invalidCases = @(
    @{ Name = 'double-to-single'; OpeningQuote = '"'; ClosingQuote = "'" }
    @{ Name = 'single-to-double'; OpeningQuote = "'"; ClosingQuote = '"' }
  )

  foreach ($case in $invalidCases) {
    $workflowPath = Join-Path $testDirectory "$($case.Name).yml"
    $fixture = New-ReleaseWorkflowFixture -OpeningQuote $case.OpeningQuote -ClosingQuote $case.ClosingQuote
    [IO.File]::WriteAllText($workflowPath, $fixture, [Text.UTF8Encoding]::new($false))

    $failedAsExpected = $false
    try {
      $null = & $updateScriptPath -ReleaseTag 'v1.2.3' -WorkflowPath $workflowPath
    }
    catch {
      $failedAsExpected = $true
    }

    Assert-True -Condition $failedAsExpected -Message "$($case.Name) quotes should be rejected."
  }

  $crateScriptPath = Join-Path $PSScriptRoot 'Set-CrateVersion.ps1'
  $cargoTomlPath = Join-Path $testDirectory 'Cargo.toml'
  $cargoLockPath = Join-Path $testDirectory 'Cargo.lock'
  [IO.File]::WriteAllText(
    $cargoTomlPath,
    '[package]' + $newline + 'name = "lastkey"' + $newline + 'version = "0.1.0"' + $newline +
    $newline + '[dependencies]' + $newline + 'serde = { version = "1.0" }' + $newline
  )
  [IO.File]::WriteAllText(
    $cargoLockPath,
    '[[package]]' + $newline + 'name = "lastkey"' + $newline + 'version = "0.1.0"' + $newline
  )

  & $crateScriptPath -Version '1.0.3' -CargoTomlPath $cargoTomlPath -CargoLockPath $cargoLockPath
  $updatedToml = [IO.File]::ReadAllText($cargoTomlPath)
  $updatedLock = [IO.File]::ReadAllText($cargoLockPath)
  Assert-True -Condition $updatedToml.Contains('version = "1.0.3"') -Message "Cargo.toml version was not updated."
  Assert-True -Condition $updatedToml.Contains('serde = { version = "1.0" }') -Message "Cargo.toml dependency version must stay untouched."
  Assert-True -Condition $updatedLock.Contains('version = "1.0.3"') -Message "Cargo.lock version was not updated."

  $failedAsExpected = $false
  try {
    & $crateScriptPath -Version 'not-a-version' -CargoTomlPath $cargoTomlPath -CargoLockPath $cargoLockPath
  }
  catch {
    $failedAsExpected = $true
  }
  Assert-True -Condition $failedAsExpected -Message "Invalid versions should be rejected."

  # Mirror of the msix/validate-msix.ps1 executable check: exact four-part
  # numeric equality, so prefix collisions fail. Keep in sync.
  function Test-FixtureBinaryVersion([int[]]$Expected, [int[]]$Actual) {
    ($Actual[0] -eq $Expected[0]) -and ($Actual[1] -eq $Expected[1]) -and
    ($Actual[2] -eq $Expected[2]) -and ($Actual[3] -eq 0)
  }

  Assert-True -Condition (Test-FixtureBinaryVersion @(1, 0, 3) @(1, 0, 3, 0)) -Message "Matching versions should be accepted."
  Assert-True -Condition (-not (Test-FixtureBinaryVersion @(1, 0, 3) @(1, 0, 30, 0))) -Message "1.0.30.0 must not match 1.0.3."
  Assert-True -Condition (-not (Test-FixtureBinaryVersion @(1, 0, 3) @(1, 0, 31, 0))) -Message "1.0.31.0 must not match 1.0.3."
}
finally {
  Remove-Item -LiteralPath $testDirectory -Recurse -Force -ErrorAction SilentlyContinue
}
