<#
Passage depuis GoXLR Mini 4.8.0 sur une machine Windows de test (GitHub Actions), comme chez un utilisateur : la
vraie 4.8.0 est installee, son updater trouve la nouvelle version sur un faux GitHub local (scripts/fake_github.py),
telecharge et installe le pont (dist\GoXLR-Mini.zip). Rustudio doit prendre la place, reprendre les reglages,
retirer GoXLR Mini, puis supprimer la tache « GoXLR Mini Update » une fois le pont termine.
A lancer avant smoke-test.ps1 : Rustudio ne reprend les reglages de GoXLR Mini qu'a sa premiere installation.
#>
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$dist = Join-Path $root 'dist'
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$legacy = Join-Path $env:ProgramFiles 'GoXLR Mini'
$legacyData = Join-Path $env:ProgramData 'GoXLR Mini'
$data = Join-Path $env:ProgramData 'Rustudio'
$api = 'http://127.0.0.1:14565'
$uninstall = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
$tmp = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }

function Show-Logs {
  Write-Host '--- GoXLR Mini'; Get-Content (Join-Path $legacyData 'goxlrctl.log') -ErrorAction SilentlyContinue | Select-Object -Last 30
  Write-Host '--- installeur'; Get-Content (Join-Path $legacyData 'update\rustudio-setup.txt') -ErrorAction SilentlyContinue
  Write-Host '--- Rustudio'; Get-Content (Join-Path $data 'rustudio.log') -ErrorAction SilentlyContinue | Select-Object -Last 30
}
function Fail([string]$msg) { Show-Logs; throw $msg }
function Health { try { Invoke-RestMethod "$api/api/health" -TimeoutSec 2 } catch { $null } }
function Wait-Until([scriptblock]$cond, [int]$seconds, [string]$what) {
  $t = [Diagnostics.Stopwatch]::StartNew()
  while ($t.Elapsed.TotalSeconds -lt $seconds) {
    $ok = try { & $cond } catch { $false }
    if ($ok) { Write-Host ("{0} ({1:N0} s)" -f $what, $t.Elapsed.TotalSeconds); return }
    Start-Sleep 1
  }
  Fail "Delai depasse : $what"
}
function Legacy-Post([string]$action, $body) {
  Invoke-RestMethod -Method Post "$api/api/$action" -Headers @{ 'X-GoXLR' = '1' } -Body ($body | ConvertTo-Json -Compress) -ContentType 'application/json'
}

if (Test-Path (Join-Path $data 'state.json')) { throw 'Rustudio est deja passe sur cette machine : lancer ce test en premier' }

# 1. la vraie GoXLR Mini 4.8.0, telle que publiee
$setup = Join-Path $tmp 'GoXLR-Mini-Setup-4.8.0.exe'
Invoke-WebRequest 'https://github.com/samirmedani/goxlr-mini/releases/download/v4.8.0/GoXLR-Mini-Setup.exe' -OutFile $setup -UseBasicParsing
if ((Get-FileHash $setup -Algorithm SHA256).Hash.ToLower() -ne 'fdfaeda4cfc09480ef99298e4e4d708f2436f39c87a150deb9e160c66d01f269') { throw 'Empreinte de GoXLR Mini 4.8.0 incorrecte' }
$p = Start-Process $setup -ArgumentList '/quiet', '/nodesktop', '/nostartup' -Wait -PassThru
if ($p.ExitCode -ne 0) { Fail "Installation de GoXLR Mini 4.8.0 echouee (code $($p.ExitCode))" }
Wait-Until { (Health).version -eq '4.8.0' } 90 'GoXLR Mini 4.8.0 en marche'

# 2. son moteur, relance pour lire les releases sur le faux GitHub (meme code, memes droits administrateur)
$fake = Start-Process python -ArgumentList ('"' + (Join-Path $root 'scripts\fake_github.py') + '"'), ('"' + $dist + '"'), $version, '8765' -PassThru -WindowStyle Hidden
Stop-ScheduledTask -TaskName 'GoXLR Mini'
Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'python*' -and $_.ExecutablePath -and $_.ExecutablePath.StartsWith("$legacy\", [StringComparison]::OrdinalIgnoreCase) } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Wait-Until { -not (Health) } 30 'Moteur 4.8.0 arrete'
$env:GOXLR_UPDATE_API = 'http://127.0.0.1:8765'
$env:GOXLR_UPDATE_PREFIX = 'http://127.0.0.1:8765/dl/'
$env:GOXLR_UPDATE_HOSTS = '127.0.0.1'
Start-Process (Join-Path $legacy 'python\pythonw.exe') -ArgumentList ('"' + (Join-Path $legacy 'goxlrctl.py') + '"'), 'daemon' -WorkingDirectory $legacy
Wait-Until { (Health).version -eq '4.8.0' } 60 'Moteur 4.8.0 relance sur le faux GitHub'

# 3. un reglage a soi, qui doit survivre au passage
Legacy-Post 'volume' @{ channel = 'Mic'; value = 123 } | Out-Null
Wait-Until { (Get-Content (Join-Path $legacyData 'state.json') -Raw | ConvertFrom-Json).volumes.Mic -eq 123 } 30 'Reglage enregistre par GoXLR Mini'

# 4. le clic « Mettre a jour » de GoXLR Mini 4.8.0
Legacy-Post 'update-check' @{} | Out-Null
Wait-Until { $u = Invoke-RestMethod "$api/api/update"; $u.status -eq 'available' -and $u.latest.version -eq $version -and $u.installable } 30 "GoXLR Mini 4.8.0 propose la $version"
Legacy-Post 'update-install' @{} | Out-Null
Wait-Until { (Health).version -eq $version } 300 'Rustudio a la place de GoXLR Mini'
Wait-Until { (Get-Content (Join-Path $legacyData 'update\result.json') -Raw | ConvertFrom-Json).version -eq $version } 180 'Pont termine'
$res = Get-Content (Join-Path $legacyData 'update\result.json') -Raw | ConvertFrom-Json
if (-not $res.ok) { Fail "Le pont signale un echec : $($res.error)" }

# 5. Rustudio en place, GoXLR Mini retire, reglages repris
if ((Get-Service Rustudio).Status -ne 'Running') { Fail 'Service Rustudio arrete' }
if (Test-Path $legacy) { Fail 'Dossier de GoXLR Mini toujours present' }
if (Get-ScheduledTask -TaskName 'GoXLR Mini' -ErrorAction SilentlyContinue) { Fail 'Tache GoXLR Mini toujours presente' }
if (Get-ItemProperty "$uninstall\GoXLRMini" -ErrorAction SilentlyContinue) { Fail 'GoXLR Mini toujours dans Applications' }
if (-not (Get-ItemProperty "$uninstall\Rustudio" -ErrorAction SilentlyContinue)) { Fail 'Rustudio absent de Applications' }
$st = Invoke-RestMethod "$api/api/state"
if ($st.state.volumes.Mic -ne 123) { Fail "Reglages non repris (Mic = $($st.state.volumes.Mic))" }
Wait-Until { (Get-Content (Join-Path $data 'state.json') -Raw | ConvertFrom-Json).volumes.Mic -eq 123 } 10 'Reglages repris enregistres par Rustudio'
Write-Host 'Rustudio en place, reglages repris, GoXLR Mini retire'

# 6. menage par Rustudio : la tache « GoXLR Mini Update » ne doit pas relancer le pont le soir meme
Wait-Until { -not (Get-ScheduledTask -TaskName 'GoXLR Mini Update' -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $legacyData 'update')) } 420 'Tache GoXLR Mini Update et fichiers temporaires retires'
if (-not (Test-Path (Join-Path $legacyData 'state.json'))) { Fail 'Anciens reglages de GoXLR Mini effaces' }
Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue
Write-Host "Passage de GoXLR Mini 4.8.0 a Rustudio $version : OK"
