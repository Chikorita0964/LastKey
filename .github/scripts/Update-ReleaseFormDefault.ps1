[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [ValidatePattern('^v\d+\.\d+\.\d+$')]
  [string]$ReleaseTag,

  [string]$WorkflowPath = '.github/workflows/release.yml'
)

$content = [IO.File]::ReadAllText($WorkflowPath)
$versionParts = $ReleaseTag.Substring(1).Split('.')
$nextReleaseTag = "v$($versionParts[0]).$($versionParts[1]).$([int]$versionParts[2] + 1)"

$currentVersionPattern = '(?m)^(        description:\s*(?<quote>["'']?))Current version: v\d+\.\d+\.\d+(\. Next release tag to create\.\k<quote>\r?)$'
$nextReleasePattern = '(?m)^(        default:\s*)v\d+\.\d+\.\d+(\r?)$'
$currentVersionMatches = [regex]::Matches($content, $currentVersionPattern)
$nextReleaseMatches = [regex]::Matches($content, $nextReleasePattern)
if ($currentVersionMatches.Count -ne 1 -or $nextReleaseMatches.Count -ne 1) {
  throw "Expected one current-version description and one next-release default in $WorkflowPath."
}

$updated = [regex]::Replace($content, $currentVersionPattern, ('${1}Current version: ' + $ReleaseTag + '${2}'))
$updated = [regex]::Replace($updated, $nextReleasePattern, ('${1}' + $nextReleaseTag + '${2}'))
[IO.File]::WriteAllText($WorkflowPath, $updated, [Text.UTF8Encoding]::new($false))

$nextReleaseTag
