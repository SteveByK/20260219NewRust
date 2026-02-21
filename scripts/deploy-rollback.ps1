param(
  [Parameter(Mandatory = $true)][string]$KnownGoodCommit,
  [string]$BranchName = "rollback-$(Get-Date -Format 'yyyyMMdd-HHmmss')",
  [switch]$Apply,
  [switch]$NoBranch,
  [switch]$AllowDirty
)

$ErrorActionPreference = "Stop"

function Invoke-Git {
  param([string[]]$GitArgs)

  $output = & git @GitArgs 2>&1
  $exitCode = $LASTEXITCODE
  return [PSCustomObject]@{
    ExitCode = $exitCode
    Output = $output
  }
}

function Require-GitOk {
  param(
    [string[]]$GitArgs,
    [string]$OnError
  )

  $result = Invoke-Git -GitArgs $GitArgs
  if ($result.ExitCode -ne 0) {
    throw "$OnError`n$($result.Output -join "`n")"
  }
  return $result.Output
}

function Get-GitFirstLine {
  param([object]$Output)

  if ($Output -is [System.Array]) {
    if ($Output.Count -eq 0) {
      return ""
    }
    return [string]$Output[0]
  }

  return [string]$Output
}

Require-GitOk -GitArgs @("rev-parse", "--is-inside-work-tree") -OnError "Current directory is not a git repository"

$currentBranch = (Get-GitFirstLine (Require-GitOk -GitArgs @("rev-parse", "--abbrev-ref", "HEAD") -OnError "Failed to resolve current branch")).Trim()
$currentHead = (Get-GitFirstLine (Require-GitOk -GitArgs @("rev-parse", "HEAD") -OnError "Failed to resolve current HEAD")).Trim()

$dirty = (Invoke-Git -GitArgs @("status", "--porcelain")).Output
if (-not $AllowDirty -and $dirty.Count -gt 0) {
  throw "Working tree has uncommitted changes. Commit/stash first or pass -AllowDirty."
}

$exists = Invoke-Git -GitArgs @("cat-file", "-e", "$KnownGoodCommit^{commit}")
if ($exists.ExitCode -ne 0) {
  throw "Known-good commit not found: $KnownGoodCommit"
}

$changedSinceGood = @(Require-GitOk -GitArgs @("diff", "--name-only", "$KnownGoodCommit..HEAD") -OnError "Failed to compute changed file list")
$commitList = @(Require-GitOk -GitArgs @("log", "--oneline", "$KnownGoodCommit..HEAD") -OnError "Failed to compute commit range")

$result = [PSCustomObject]@{
  mode = if ($Apply) { "apply" } else { "preview" }
  currentBranch = $currentBranch
  currentHead = $currentHead
  knownGoodCommit = $KnownGoodCommit
  commitCountToRevert = $commitList.Count
  filesChangedSinceKnownGood = $changedSinceGood
  suggestedBranch = $BranchName
  nextSteps = @(
    "railway rollback is commit-based: deploy a reverted commit",
    "after push, redeploy platform and verify /health /ready /"
  )
}

if (-not $Apply) {
  Write-Host "Rollback preview (no repo changes made)" -ForegroundColor Yellow
  $result | ConvertTo-Json -Depth 10
  exit 0
}

if ($commitList.Count -eq 0) {
  Write-Host "HEAD already matches known-good commit range; nothing to revert." -ForegroundColor Green
  $result | ConvertTo-Json -Depth 10
  exit 0
}

if (-not $NoBranch) {
  Require-GitOk -GitArgs @("checkout", "-b", $BranchName) -OnError "Failed to create rollback branch"
}

$revertRange = "$KnownGoodCommit..HEAD"
$revert = Invoke-Git -GitArgs @("revert", "--no-edit", "--no-commit", $revertRange)
if ($revert.ExitCode -ne 0) {
  Invoke-Git -GitArgs @("revert", "--abort") | Out-Null
  throw "git revert failed. Resolve conflicts manually and retry.`n$($revert.Output -join "`n")"
}

$hasDiff = (Invoke-Git -GitArgs @("status", "--porcelain")).Output
if ($hasDiff.Count -eq 0) {
  Write-Host "No staged changes after revert; nothing to commit." -ForegroundColor Yellow
  $result | ConvertTo-Json -Depth 10
  exit 0
}

$commitMessage = "chore(rollback): revert to known-good $KnownGoodCommit"
Require-GitOk -GitArgs @("commit", "-m", $commitMessage) -OnError "Failed to commit rollback"

$finalHead = (Get-GitFirstLine (Require-GitOk -GitArgs @("rev-parse", "HEAD") -OnError "Failed to resolve rollback head")).Trim()

$applied = [PSCustomObject]@{
  status = "rollback-commit-created"
  branch = if ($NoBranch) { $currentBranch } else { $BranchName }
  rollbackHead = $finalHead
  pushCommand = if ($NoBranch) { "git push" } else { "git push -u origin $BranchName" }
  verifyCommands = @(
    "powershell -ExecutionPolicy Bypass -File ./scripts/deploy-preflight.ps1 -BaseUrl https://<platform-url>",
    "curl https://<platform-url>/health",
    "curl https://<platform-url>/ready",
    "curl https://<platform-url>/"
  )
}

Write-Host "Rollback commit created" -ForegroundColor Green
$applied | ConvertTo-Json -Depth 10
exit 0
