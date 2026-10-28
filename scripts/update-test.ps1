<#
Mise a jour integree de Rustudio sur une machine Windows de test (GitHub Actions), comme chez un utilisateur : une
version suivante est fabriquee (meme code, numero + 1) et servie par un faux GitHub local (scripts/fake_github.py).
Le moteur installe doit la proposer, l'installer au clic (tache « Rustudio Update »), redemarrer dessus, garder les
reglages et faire le menage. A lancer apres migration-test.ps1 (Rustudio installe), avant smoke-test.ps1.
#>
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$api = 'http://127.0.0.1:14565'
$data = Join-Path $env:ProgramData 'Rustudio'
$dir = Join-Path $env:ProgramFiles 'Rustudio'
$next_dist = Join-Path $root 'dist-next'
$svcKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\Rustudio'
$uninstall = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Rustudio'

function Show-Log { Get-Content (Join-Path $data 'rustudio.log') -ErrorAction SilentlyContinue | Select-Object -Last 40 }
function Fail([string]$msg) { Show-Log; throw $msg }
function Health { try { Invoke-RestMethod "$api/api/health" -TimeoutSec 2 } catch { $null } }
function Update { Invoke-RestMethod "$api/api/update" -TimeoutSec 3 }
function Wait-Until([scriptblock]$cond, [int]$seconds, [string]$what) {
  $t = [Diagnostics.Stopwatch]::StartNew()
  while ($t.Elapsed.TotalSeconds -lt $seconds) {
    $ok = try { & $cond } catch { $false }
    if ($ok) { Write-Host ("{0} ({1:N0} s)" -f $what, $t.Elapsed.TotalSeconds); return }
    Start-Sleep 1
  }
  Fail "Delai depasse : $what"
}
function Post([string]$action) {
  Invoke-RestMethod -Method Post "$api/api/$action" -Headers @{ 'X-Rustudio' = '1' } -Body '{}' -ContentType 'application/json'
}

$cur = (Health).version
if (-not $cur) { throw 'Rustudio ne tourne pas : lancer migration-test.ps1 avant' }
$v = [version]$cur
$next = '{0}.{1}.{2}' -f $v.Major, $v.Minor, ($v.Build + 1)
$micBefore = (Invoke-RestMethod "$api/api/state").state.volumes.Mic

# 1. la version suivante : meme code, numero + 1 (Cargo.toml et Cargo.lock remis comme avant ensuite)
Push-Location $root
try {
  $toml = [IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'))
  $old = 'version = "' + $cur + '"'
  $i = $toml.IndexOf($old)
  if ($i -lt 0) { throw "Version $cur introuvable dans Cargo.toml" }
  [IO.File]::WriteAllText((Join-Path $root 'Cargo.toml'), $toml.Substring(0, $i) + 'version = "' + $next + '"' + $toml.Substring($i + $old.Length), (New-Object Text.UTF8Encoding $false))
  cargo update --workspace
  if ($LASTEXITCODE -ne 0) { throw 'cargo update --workspace echoue' }
  ./scripts/build-release.ps1 -Out $next_dist -PayloadOnly
} finally {
  git checkout -- Cargo.toml Cargo.lock
  Pop-Location
}

# 2. le service lit les releases sur le faux GitHub
$fake = Start-Process python -ArgumentList ('"' + (Join-Path $root 'scripts\fake_github.py') + '"'), ('"' + $next_dist + '"'), $next, '8766', 'Rustudio.zip', 'Rustudio.sha256' -PassThru -WindowStyle Hidden
New-ItemProperty $svcKey -Name Environment -PropertyType MultiString -Force -Value @(
  'RUSTUDIO_UPDATE_API=http://127.0.0.1:8766', 'RUSTUDIO_UPDATE_PREFIX=http://127.0.0.1:8766/dl/', 'RUSTUDIO_UPDATE_HOSTS=127.0.0.1') | Out-Null
Restart-Service Rustudio
Wait-Until { (Health).version -eq $cur } 30 "Rustudio $cur relance sur le faux GitHub"

# 3. « Mise a jour proposee », puis le clic
try { Post 'update-check' | Out-Null } catch { }
Wait-Until { $u = Update; $u.status -eq 'available' -and $u.latest.version -eq $next -and $u.installable } 60 "Rustudio $cur propose la $next"
Post 'update-install' | Out-Null
Wait-Until { (Health).version -eq $next } 180 "Rustudio $next en marche"
# l'appli ouverte affiche l'avis « installee » puis l'efface : le journal du moteur fait foi
$logf = Join-Path $data 'rustudio.log'
Wait-Until { Select-String -Path $logf -SimpleMatch "Version $next installée" -Quiet } 60 'Installation terminee'
if (Select-String -Path $logf -SimpleMatch "Échec, retour à $cur" -Quiet) { Fail 'Retour arriere pendant la mise a jour' }

# 4. fichiers, version affichee, sauvegarde de l'ancienne version, reglages
if ((Get-Content (Join-Path $dir 'manifest.json') -Raw | ConvertFrom-Json).version -ne $next) { Fail 'Manifeste installe pas a jour' }
if ((Get-ItemProperty $uninstall).DisplayVersion -ne $next) { Fail 'Version affichee dans Applications pas a jour' }
if (-not (Get-ChildItem (Join-Path $dir 'backup') -Directory -Filter "*-$cur" -ErrorAction SilentlyContinue)) { Fail "Pas de sauvegarde de la $cur" }
if ((Invoke-RestMethod "$api/api/state").state.volumes.Mic -ne $micBefore) { Fail 'Reglages perdus pendant la mise a jour' }
if ((Get-Service Rustudio).Status -ne 'Running') { Fail 'Service Rustudio arrete' }
Write-Host "Rustudio $cur -> $next : fichiers, version, sauvegarde et reglages OK"

# 5. menage : tache d'installation et copie preparee retirees
Wait-Until { -not (Get-ScheduledTask -TaskName 'Rustudio Update' -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $data 'update\new')) } 120 'Tache Rustudio Update et copie preparee retirees'

Remove-ItemProperty $svcKey -Name Environment -ErrorAction SilentlyContinue
Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue
Write-Host "Mise a jour integree $cur -> $next : OK"
