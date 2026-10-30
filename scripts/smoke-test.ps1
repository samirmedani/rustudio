<#
Installation complete sur une machine Windows de test (GitHub Actions) : installeur silencieux, service, moteur,
interface, appli, puis desinstallation. Sans GoXLR : le moteur doit tourner et attendre l'appareil.
#>
$ErrorActionPreference = 'Stop'
$dist = (Resolve-Path (Join-Path $PSScriptRoot '..\dist')).Path
$dir = Join-Path $env:ProgramFiles 'Rustudio'
$data = Join-Path $env:ProgramData 'Rustudio'
function Show-Log { Get-Content (Join-Path $data 'rustudio.log') -ErrorAction SilentlyContinue | Select-Object -Last 40 }

$p = Start-Process (Join-Path $dist 'Rustudio-Setup.exe') -ArgumentList '--quiet', '--no-launch', '--no-desktop', '--no-startup' -Wait -PassThru
if ($p.ExitCode -ne 0) { Show-Log; throw "Installation echouee (code $($p.ExitCode))" }
$svc = Get-Service Rustudio
if ($svc.Status -ne 'Running') { Show-Log; throw "Service $($svc.Status)" }
$h = Invoke-RestMethod http://127.0.0.1:14565/api/health
Write-Host "Moteur $($h.version), statut $($h.status), supervise : $($h.supervised)"
if (-not $h.supervised) { throw 'Le moteur ne tourne pas sous son superviseur' }
$st = Invoke-RestMethod http://127.0.0.1:14565/api/state
if (-not $st.meta.channels) { throw 'Etat illisible' }
$ui = Invoke-WebRequest http://127.0.0.1:14565/ -UseBasicParsing
if ($ui.Content -notmatch 'Rustudio') { throw 'Interface absente' }
$code = try { (Invoke-WebRequest -Method Post http://127.0.0.1:14565/api/volume -Body '{"channel":"Mic","value":1}' -ContentType 'application/json' -UseBasicParsing).StatusCode } catch { $_.Exception.Response.StatusCode.value__ }
if ($code -ne 403) { throw "POST sans en-tete accepte ($code)" }
$r = Invoke-RestMethod -Method Post http://127.0.0.1:14565/api/volume -Headers @{ 'X-Rustudio' = '1' } -Body '{"channel":"Mic","value":200}' -ContentType 'application/json'
if (-not $r.ok) { throw 'Action refusee' }
$sp = Join-Path $data 'state.json'
function Saved-Mic { try { (Get-Content $sp -Raw | ConvertFrom-Json).volumes.Mic } catch { $null } }
for ($i = 0; $i -lt 20 -and (Saved-Mic) -ne 200; $i++) { Start-Sleep -Milliseconds 500 }
if ((Saved-Mic) -ne 200) { Show-Log; throw 'Reglage jamais enregistre sur le disque' }
if (-not (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Rustudio' -ErrorAction SilentlyContinue)) { throw 'Entree Applications absente' }
# journal Windows des coupures du son : allume par le moteur (compte systeme) et lisible par l'API
$g = $null
for ($i = 0; $i -lt 20 -and -not ($g -and $g.enabled); $i++) { Start-Sleep -Milliseconds 500; $g = Invoke-RestMethod http://127.0.0.1:14565/api/glitches }
if (-not $g.enabled) { Show-Log; throw 'Journal des coupures du son non allume' }
if (-not $g.readable -or $null -eq $g.total) { Show-Log; throw "Journal des coupures illisible : $($g.error)" }
[xml]$channelConfig = wevtutil gl Microsoft-Windows-Audio/GlitchDetection /f:xml
if ($channelConfig.DocumentElement.GetAttribute('enabled') -ne 'true') { throw 'Canal GlitchDetection eteint' }
Write-Host "Journal des coupures : allume, $($g.total) coupure(s) sur 24 h"
# test « son qui crepite » : isolement temporaire de la voie Sample, puis retour
$r = Invoke-RestMethod -Method Post http://127.0.0.1:14565/api/sound-test -Headers @{ 'X-Rustudio' = '1' } -Body '{"seconds":30}' -ContentType 'application/json'
if (-not $r.ok -or -not (Invoke-RestMethod http://127.0.0.1:14565/api/live).sound_test) { throw 'Test du son : isolement de Sample refuse' }
Invoke-RestMethod -Method Post http://127.0.0.1:14565/api/sound-test -Headers @{ 'X-Rustudio' = '1' } -Body '{"seconds":0}' -ContentType 'application/json' | Out-Null
if ((Invoke-RestMethod http://127.0.0.1:14565/api/live).sound_test) { throw 'Test du son : Sample pas rendu' }

# moteur tue : le superviseur le relance
$eng = Get-CimInstance Win32_Process -Filter "Name='rustudio-engine.exe'" | Where-Object { $_.CommandLine -match '--child' }
Stop-Process -Id $eng.ProcessId -Force
$back = $false
for ($i = 0; $i -lt 30; $i++) { Start-Sleep 1; try { $h2 = Invoke-RestMethod http://127.0.0.1:14565/api/health -TimeoutSec 2; if ($h2.engine_restarts -ge 1) { $back = $true; break } } catch { } }
if (-not $back) { Show-Log; throw 'Le moteur ne revient pas apres un arret brutal' }
Write-Host 'Moteur relance par son superviseur'

# l'appli demarre et reste ouverte
$app = Start-Process (Join-Path $dir 'Rustudio.exe') -ArgumentList '--tray' -PassThru
Start-Sleep 10
if ($app.HasExited) { throw "L'appli s'est fermee au demarrage (code $($app.ExitCode))" }
Stop-Process -Id $app.Id -Force
Write-Host 'Appli ouverte sans erreur'

# desinstallation
Start-Process (Join-Path $dir 'rustudio-engine.exe') -ArgumentList 'uninstall', '--quiet' -Wait
for ($i = 0; $i -lt 40 -and ((Get-Service Rustudio -ErrorAction SilentlyContinue) -or (Test-Path (Join-Path $dir 'Rustudio.exe'))); $i++) { Start-Sleep 1 }
if (Get-Service Rustudio -ErrorAction SilentlyContinue) { throw 'Service toujours present apres desinstallation' }
if (Test-Path (Join-Path $dir 'Rustudio.exe')) { throw 'Fichiers toujours presents apres desinstallation' }
if ((Saved-Mic) -ne 200) { throw 'Reglages perdus a la desinstallation' }
Write-Host 'Installation, service, moteur, interface, appli et desinstallation : OK'
