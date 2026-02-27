param(
  [string]$BaseUrl = "http://127.0.0.1:3000",
  [string]$EnvFile,
  [switch]$SkipEnvCheck,
  [switch]$SkipTcpCheck,
  [switch]$SkipEndpointCheck,
  [int]$TimeoutSeconds = 5
)

$ErrorActionPreference = "Stop"

function Parse-EnvFile {
  param([string]$Path)

  $values = @{}
  if ([string]::IsNullOrWhiteSpace($Path)) {
    return $values
  }

  if (-not (Test-Path $Path)) {
    throw "Env file not found: $Path"
  }

  foreach ($line in Get-Content $Path) {
    $trimmed = $line.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed) -or $trimmed.StartsWith("#")) {
      continue
    }

    $idx = $trimmed.IndexOf("=")
    if ($idx -lt 1) {
      continue
    }

    $key = $trimmed.Substring(0, $idx).Trim()
    $value = $trimmed.Substring($idx + 1).Trim().Trim('"')
    if (-not [string]::IsNullOrWhiteSpace($key)) {
      $values[$key] = $value
    }
  }

  return $values
}

function Resolve-ConfigValue {
  param(
    [string]$Name,
    [hashtable]$FileVars
  )

  if ($null -ne $FileVars -and $FileVars.ContainsKey($Name)) {
    return $FileVars[$Name]
  }

  return [Environment]::GetEnvironmentVariable($Name)
}

function Get-UriParts {
  param([string]$Value)

  if ([string]::IsNullOrWhiteSpace($Value)) {
    return $null
  }

  try {
    $uri = [System.Uri]$Value
    return [PSCustomObject]@{
      Hostname = $uri.Host
      Port = $uri.Port
      Value = $Value
    }
  } catch {
    return $null
  }
}

function Test-TcpEndpoint {
  param(
    [string]$Name,
    [string]$TargetAddress,
    [int]$Port,
    [int]$TimeoutSeconds = 5
  )

  $client = New-Object System.Net.Sockets.TcpClient
  try {
    $iar = $client.BeginConnect($TargetAddress, $Port, $null, $null)
    if (-not $iar.AsyncWaitHandle.WaitOne([TimeSpan]::FromSeconds($TimeoutSeconds))) {
      $client.Close()
      return [PSCustomObject]@{
        name = $Name
        host = $TargetAddress
        port = $Port
        ok = $false
        message = "connect timeout"
      }
    }

    $client.EndConnect($iar)
    $client.Close()
    return [PSCustomObject]@{
      name = $Name
      host = $TargetAddress
      port = $Port
      ok = $true
      message = "ok"
    }
  } catch {
    return [PSCustomObject]@{
      name = $Name
      host = $TargetAddress
      port = $Port
      ok = $false
      message = $_.Exception.Message
    }
  }
}

function Invoke-EndpointStatus {
  param(
    [string]$Url,
    [int]$TimeoutSeconds = 5
  )

  try {
    $resp = Invoke-WebRequest -UseBasicParsing -Uri $Url -Method GET -TimeoutSec $TimeoutSeconds
    $body = ($resp.Content | Out-String).Trim()
    $bodySample = $body.Substring(0, [Math]::Min(120, $body.Length))
    $isStaticFallback = ($body -like "*Platform is running*" -and $body -like "*Static assets were not generated in this build.*")
    return [PSCustomObject]@{
      url = $Url
      status = [int]$resp.StatusCode
      ok = ([int]$resp.StatusCode -ge 200 -and [int]$resp.StatusCode -lt 300)
      bodySample = $bodySample
      isStaticFallback = $isStaticFallback
    }
  } catch {
    $status = 0
    $body = ""
    if ($_.Exception.Response -and $_.Exception.Response.StatusCode) {
      $status = [int]$_.Exception.Response.StatusCode
      try {
        $stream = $_.Exception.Response.GetResponseStream()
        if ($null -ne $stream) {
          $reader = New-Object System.IO.StreamReader($stream)
          $body = $reader.ReadToEnd()
        }
      } catch {
      }
    }

    return [PSCustomObject]@{
      url = $Url
      status = $status
      ok = $false
      bodySample = (($body | Out-String).Trim().Substring(0, [Math]::Min(120, (($body | Out-String).Trim().Length))) )
      isStaticFallback = $false
      error = $_.Exception.Message
    }
  }
}

$requiredVars = @(
  "DATABASE_URL",
  "REDIS_URL",
  "NATS_URL",
  "CLICKHOUSE_URL",
  "JWT_SECRET"
)

$fileVars = Parse-EnvFile -Path $EnvFile
$missing = @()
$resolved = @{}

foreach ($name in $requiredVars) {
  $value = Resolve-ConfigValue -Name $name -FileVars $fileVars
  $resolved[$name] = $value
  if ([string]::IsNullOrWhiteSpace($value)) {
    $missing += $name
  }
}

$tcpChecks = @()
if (-not $SkipTcpCheck) {
  $targets = @(
    [PSCustomObject]@{ name = "postgres"; var = "DATABASE_URL"; fallbackPort = 5432 },
    [PSCustomObject]@{ name = "redis"; var = "REDIS_URL"; fallbackPort = 6379 },
    [PSCustomObject]@{ name = "nats"; var = "NATS_URL"; fallbackPort = 4222 },
    [PSCustomObject]@{ name = "clickhouse"; var = "CLICKHOUSE_URL"; fallbackPort = 8123 }
  )

  foreach ($target in $targets) {
    $value = $resolved[$target.var]
    $parts = Get-UriParts -Value $value
    if ($null -eq $parts) {
      $tcpChecks += [PSCustomObject]@{
        name = $target.name
        host = ""
        port = $target.fallbackPort
        ok = $false
        message = "invalid or missing URI in $($target.var)"
      }
      continue
    }

    $port = $parts.Port
    if ($port -le 0) {
      $port = $target.fallbackPort
    }

    $tcpChecks += Test-TcpEndpoint -Name $target.name -TargetAddress $parts.Hostname -Port $port -TimeoutSeconds $TimeoutSeconds
  }
}

$endpointChecks = @()
if (-not $SkipEndpointCheck) {
  $endpointChecks += Invoke-EndpointStatus -Url "$BaseUrl/health" -TimeoutSeconds $TimeoutSeconds
  $endpointChecks += Invoke-EndpointStatus -Url "$BaseUrl/ready" -TimeoutSeconds $TimeoutSeconds
  $endpointChecks += Invoke-EndpointStatus -Url "$BaseUrl/" -TimeoutSeconds $TimeoutSeconds
}

$errors = @()

if ((-not $SkipEnvCheck) -and $missing.Count -gt 0) {
  $errors += "Missing required vars: $($missing -join ', ')"
}

if (-not $SkipTcpCheck) {
  $tcpFailed = @($tcpChecks | Where-Object { -not $_.ok })
  if ($tcpFailed.Count -gt 0) {
    $errors += "Dependency TCP checks failed: $($tcpFailed.name -join ', ')"
  }
}

if (-not $SkipEndpointCheck) {
  $endpointFailed = @($endpointChecks | Where-Object { -not $_.ok })
  if ($endpointFailed.Count -gt 0) {
    $errors += "Endpoint checks failed: $($endpointFailed.url -join ', ')"
  }

  $fallbackPages = @($endpointChecks | Where-Object { $_.isStaticFallback })
  if ($fallbackPages.Count -gt 0) {
    $errors += "Endpoint checks failed: static fallback page detected on $($fallbackPages.url -join ', ')"
  }
}

$result = [PSCustomObject]@{
  status = if ($errors.Count -eq 0) { "ok" } else { "fail" }
  baseUrl = $BaseUrl
  envSource = if ([string]::IsNullOrWhiteSpace($EnvFile)) { "process" } else { $EnvFile }
  missingVars = $missing
  tcpChecks = $tcpChecks
  endpointChecks = $endpointChecks
  errors = $errors
}

if ($errors.Count -eq 0) {
  Write-Host "Preflight passed" -ForegroundColor Green
  $result | ConvertTo-Json -Depth 10
  exit 0
}

Write-Host "Preflight failed" -ForegroundColor Red
$result | ConvertTo-Json -Depth 10
exit 1
