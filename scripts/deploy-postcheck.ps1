param(
  [Parameter(Mandatory = $true)]
  [string]$BaseUrl,
  [int]$TimeoutSeconds = 8,
  [string]$ReportPath = "",
  [switch]$SkipRootCheck
)

$ErrorActionPreference = "Stop"

function New-ReportPath {
  param([string]$InputPath)

  if (-not [string]::IsNullOrWhiteSpace($InputPath)) {
    return $InputPath
  }

  $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $dir = Join-Path -Path "." -ChildPath "artifacts/deploy"
  if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
  }

  return (Join-Path -Path $dir -ChildPath "postcheck-$timestamp.json")
}

function Invoke-Endpoint {
  param(
    [string]$Url,
    [int[]]$ExpectedStatus,
    [int]$TimeoutSec
  )

  try {
    $response = Invoke-WebRequest -UseBasicParsing -Uri $Url -Method GET -TimeoutSec $TimeoutSec
    $statusCode = [int]$response.StatusCode
    $content = ($response.Content | Out-String).Trim()
    $sample = if ([string]::IsNullOrEmpty($content)) { "" } else { $content.Substring(0, [Math]::Min(200, $content.Length)) }
    $ok = $ExpectedStatus -contains $statusCode

    return [PSCustomObject]@{
      url = $Url
      status = $statusCode
      ok = $ok
      expected = $ExpectedStatus
      bodySample = $sample
      error = ""
    }
  } catch {
    $statusCode = 0
    $body = ""
    if ($_.Exception.Response) {
      try {
        $statusCode = [int]$_.Exception.Response.StatusCode
      } catch {}

      try {
        $stream = $_.Exception.Response.GetResponseStream()
        if ($null -ne $stream) {
          $reader = New-Object System.IO.StreamReader($stream)
          $body = $reader.ReadToEnd()
          $reader.Dispose()
        }
      } catch {}
    }

    $content = ($body | Out-String).Trim()
    $sample = if ([string]::IsNullOrEmpty($content)) { "" } else { $content.Substring(0, [Math]::Min(200, $content.Length)) }
    $ok = $ExpectedStatus -contains $statusCode

    return [PSCustomObject]@{
      url = $Url
      status = $statusCode
      ok = $ok
      expected = $ExpectedStatus
      bodySample = $sample
      error = $_.Exception.Message
    }
  }
}

$normalizedBase = $BaseUrl.TrimEnd('/')
if (-not $normalizedBase.StartsWith("http://") -and -not $normalizedBase.StartsWith("https://")) {
  throw "BaseUrl must start with http:// or https://"
}

$checks = @(
  [PSCustomObject]@{ path = "/health"; expected = @(200) },
  [PSCustomObject]@{ path = "/ready"; expected = @(200) },
  [PSCustomObject]@{ path = "/api/public-map-config"; expected = @(200) }
)

if (-not $SkipRootCheck) {
  $checks += [PSCustomObject]@{ path = "/"; expected = @(200) }
}

$results = @()
foreach ($check in $checks) {
  $url = "$normalizedBase$($check.path)"
  $results += Invoke-Endpoint -Url $url -ExpectedStatus $check.expected -TimeoutSec $TimeoutSeconds
}

$failed = @($results | Where-Object { -not $_.ok })
$report = [PSCustomObject]@{
  status = if ($failed.Count -eq 0) { "pass" } else { "fail" }
  baseUrl = $normalizedBase
  executedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
  timeoutSeconds = $TimeoutSeconds
  checks = $results
  failedCount = $failed.Count
}

$finalReportPath = New-ReportPath -InputPath $ReportPath
$reportJson = $report | ConvertTo-Json -Depth 10
Set-Content -Path $finalReportPath -Value $reportJson -Encoding UTF8

if ($failed.Count -eq 0) {
  Write-Host "Post-deploy check passed" -ForegroundColor Green
  Write-Host "Report: $finalReportPath"
  $reportJson
  exit 0
}

Write-Host "Post-deploy check failed" -ForegroundColor Red
Write-Host "Report: $finalReportPath"
$reportJson
exit 1
