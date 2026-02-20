param(
  [string]$BaseUrl = "http://127.0.0.1:3000",
  [string]$RoomId = "global",
  [int]$HealthRetry = 30,
  [int]$HealthRetryDelaySeconds = 2
)

$ErrorActionPreference = "Stop"

function Invoke-Api {
  param(
    [Parameter(Mandatory = $true)][string]$Method,
    [Parameter(Mandatory = $true)][string]$Path,
    [object]$Body
  )

  $uri = "$BaseUrl$Path"
  if ($null -eq $Body) {
    return Invoke-RestMethod -Method $Method -Uri $uri -ContentType "application/json"
  }

  $json = $Body | ConvertTo-Json -Depth 10
  return Invoke-RestMethod -Method $Method -Uri $uri -ContentType "application/json" -Body $json
}

function UrlEncode {
  param([Parameter(Mandatory = $true)][string]$Value)
  return [uri]::EscapeDataString($Value)
}

function Ensure-Auth {
  param(
    [string]$Username,
    [string]$Password
  )

  $login = Invoke-Api -Method "POST" -Path "/api/login" -Body @{ username = $Username; password = $Password }
  if ($null -ne $login -and -not [string]::IsNullOrWhiteSpace($login.token)) {
    return $login
  }

  $register = Invoke-Api -Method "POST" -Path "/api/register" -Body @{ username = $Username; password = $Password }
  if ($null -eq $register -or [string]::IsNullOrWhiteSpace($register.token)) {
    throw "Failed to get token for user: $Username"
  }
  return $register
}

function Require-Status {
  param(
    [string]$Method,
    [string]$Path,
    [object]$Body,
    [int]$Expected = 202
  )

  $uri = "$BaseUrl$Path"
  $json = $Body | ConvertTo-Json -Depth 10
  $response = Invoke-WebRequest -UseBasicParsing -Method $Method -Uri $uri -ContentType "application/json" -Body $json
  if ($response.StatusCode -ne $Expected) {
    throw "Request $Path returned status $($response.StatusCode), expected $Expected"
  }
}

Write-Host "[1/7] Health check..." -ForegroundColor Cyan
for ($i = 1; $i -le $HealthRetry; $i++) {
  try {
    $health = Invoke-Api -Method "GET" -Path "/health"
    if ($health.status -eq "ok") {
      break
    }
  } catch {
    if ($i -eq $HealthRetry) {
      throw
    }
    Start-Sleep -Seconds $HealthRetryDelaySeconds
  }
}

if ($health.status -ne "ok") {
  throw "Health check failed"
}

$aliceUser = "alice_demo"
$bobUser = "bob_demo"
$passwd = "demo-pass-2026"

Write-Host "[2/7] Auth users..." -ForegroundColor Cyan
$alice = Ensure-Auth -Username $aliceUser -Password $passwd
$bob = Ensure-Auth -Username $bobUser -Password $passwd

Write-Host "[3/7] Send chat message (Alice)..." -ForegroundColor Cyan
Require-Status -Method "POST" -Path "/api/chat/send" -Body @{
  token = $alice.token
  room_id = $RoomId
  text = "hello-from-alice-$(Get-Date -Format 'HHmmss')"
}

Write-Host "[4/7] Read room state + mark read (Bob)..." -ForegroundColor Cyan
$roomStateBefore = Invoke-Api -Method "GET" -Path "/api/chat/room-state?token=$(UrlEncode -Value $bob.token)&room_id=$(UrlEncode -Value $RoomId)"
Require-Status -Method "POST" -Path "/api/chat/mark-read" -Body @{
  token = $bob.token
  room_id = $RoomId
}
$roomStateAfter = Invoke-Api -Method "GET" -Path "/api/chat/room-state?token=$(UrlEncode -Value $bob.token)&room_id=$(UrlEncode -Value $RoomId)"

Write-Host "[5/7] Send invite (Alice -> Bob)..." -ForegroundColor Cyan
Require-Status -Method "POST" -Path "/api/invite/send" -Body @{
  token = $alice.token
  to_user = $bob.user_id
  mode = "duel"
}

Write-Host "[6/7] Load pending + accept invite (Bob)..." -ForegroundColor Cyan
$pending = Invoke-Api -Method "GET" -Path "/api/invite/pending?token=$(UrlEncode -Value $bob.token)"
if ($null -eq $pending -or $pending.Count -eq 0) {
  throw "No pending invite found for Bob"
}

$invite = $pending[0]
Require-Status -Method "POST" -Path "/api/invite/respond" -Body @{
  token = $bob.token
  invite_id = $invite.invite_id
  action = "accept"
}

Write-Host "[7/7] Verify pending list cleaned..." -ForegroundColor Cyan
$pendingAfter = Invoke-Api -Method "GET" -Path "/api/invite/pending?token=$(UrlEncode -Value $bob.token)"

$result = [PSCustomObject]@{
  health = $health.status
  room = $RoomId
  alice = [PSCustomObject]@{ user = $aliceUser; id = $alice.user_id }
  bob = [PSCustomObject]@{ user = $bobUser; id = $bob.user_id }
  unreadBefore = $roomStateBefore.unread_count
  unreadAfter = $roomStateAfter.unread_count
  inviteProcessed = $invite.invite_id
  pendingAfterCount = @($pendingAfter).Count
}

Write-Host "E2E flow complete" -ForegroundColor Green
$result | ConvertTo-Json -Depth 10
