<#
Mise a jour depuis une release deja publiee, sur une machine Windows de test (GitHub Actions), comme chez un
utilisateur : la version qu'il a (installeur de la release, telecharge sur GitHub) est installee, puis son propre
updater propose la version suivante et l'installe au clic. Verifie que la nouvelle version tourne, que les reglages
restent, que l'ancienne est sauvegardee et que le menage est fait, puis desinstalle tout.

  release-update-test.ps1                 cible = paquet construit ici (dist, ou dist-next s'il faut un numero plus
                                          haut que la derniere release), servi par le faux GitHub ; depart = derniere
                                          release publiee. Avant publication : la version d'aujourd'hui sait-elle
                                          installer la nouvelle ?
  release-update-test.ps1 -Real           cible = la vraie derniere release de GitHub, sans faux GitHub ; depart =
                                          la release d'avant. Apres publication : le chemin exact des utilisateurs.
  -From vX.Y.Z                            release de depart imposee.

A lancer sur une machine sans Rustudio installe (apres smoke-test.ps1, qui desinstalle).
Code de sortie 3 : GitHub limite les requetes de la machine de test, resultat non concluant.
#>
param([string]$From = '', [switch]$Real)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$repo = if ($env:GITHUB_REPOSITORY) { $env:GITHUB_REPOSITORY } else { 'samirmedani/rustudio' }
$api = 'http://127.0.0.1:14565'
$data = Join-Path $env:ProgramData 'Rustudio'
$dir = Join-Path $env:ProgramFiles 'Rustudio'
$svcKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\Rustudio'
$uninstall = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Rustudio'
$tmp = Join-Path $(if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }) 'rustudio-from'
$gh = @{ 'User-Agent' = 'rustudio-ci'; 'Accept' = 'application/vnd.github+json' }
if ($env:GH_TOKEN) { $gh['Authorization'] = "Bearer $($env:GH_TOKEN)" }

function Show-Log { Write-Host '--- Rustudio'; Get-Content (Join-Path $data 'rustudio.log') -ErrorAction SilentlyContinue | Select-Object -Last 40 }
function Fail([string]$msg) { Show-Log; throw $msg }
function Health { try { Invoke-RestMethod "$api/api/health" -TimeoutSec 2 } catch { $null } }
function Update { Invoke-RestMethod "$api/api/update" -TimeoutSec 3 }
function Post([string]$action, [string]$body = '{}') {
  Invoke-RestMethod -Method Post "$api/api/$action" -Headers @{ 'X-Rustudio' = '1' } -Body $body -ContentType 'application/json'
}
function Wait-Until([scriptblock]$cond, [int]$seconds, [string]$what) {
  $t = [Diagnostics.Stopwatch]::StartNew()
  while ($t.Elapsed.TotalSeconds -lt $seconds) {
    $ok = try { & $cond } catch { $false }
    if ($ok) { Write-Host ("{0} ({1:N0} s)" -f $what, $t.Elapsed.TotalSeconds); return }
    Start-Sleep 1
  }
  Fail "Delai depasse : $what"
}
function Ver([string]$tag) { [version]($tag.TrimStart('v')) }

if (Get-Service Rustudio -ErrorAction SilentlyContinue) { throw 'Rustudio deja installe : lancer ce test apres smoke-test.ps1' }

# 1. les releases publiees (publiees, ni brouillon ni pre-version), de la plus recente a la plus ancienne
$all = Invoke-RestMethod "https://api.github.com/repos/$repo/releases?per_page=30" -Headers $gh
$rels = @($all | Where-Object { -not $_.draft -and -not $_.prerelease -and $_.tag_name -match '^v\d+\.\d+\.\d+$' } |
  Sort-Object { Ver $_.tag_name } -Descending)
if (-not $rels) { throw "Aucune release publiee sur $repo" }

# 2. cible et depart
$fake = $null
if ($Real) {
  $target = $rels[0].tag_name.TrimStart('v')
  if (-not $From) {
    $prev = $rels | Where-Object { (Ver $_.tag_name) -lt (Ver $target) } | Select-Object -First 1
    if (-not $prev) { Write-Host "Une seule release publiee ($target) : rien a mettre a jour."; exit 0 }
    $From = $prev.tag_name
  }
} else {
  if (-not $From) { $From = $rels[0].tag_name }
  $cur = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  if ([version]$cur -gt (Ver $From)) {
    $target = $cur; $serve = Join-Path $root 'dist'
  } else {
    # le code n'a pas encore change de numero : paquet « numero + 1 » fabrique par update-test.ps1
    $v = [version]$cur
    $target = '{0}.{1}.{2}' -f $v.Major, $v.Minor, ($v.Build + 1)
    $serve = Join-Path $root 'dist-next'
  }
  if (-not (Test-Path (Join-Path $serve 'Rustudio.zip'))) { throw "Paquet $target introuvable dans $serve" }
  if ([version]$target -le (Ver $From)) { throw "La cible $target n'est pas plus recente que $From" }
}
$fromV = $From.TrimStart('v')
if (-not ($rels | Where-Object { $_.tag_name -eq "v$fromV" })) { throw "Release v$fromV introuvable sur $repo" }
Write-Host "Depart : Rustudio $fromV (release publiee) -> cible : $target $(if ($Real) { '(vraie release GitHub)' } else { '(faux GitHub local)' })"

# 3. installer la version de depart, avec son propre installeur
New-Item -ItemType Directory -Force $tmp | Out-Null
$setup = Join-Path $tmp "Rustudio-Setup-$fromV.exe"
Invoke-WebRequest "https://github.com/$repo/releases/download/v$fromV/Rustudio-Setup.exe" -OutFile $setup -UseBasicParsing
$p = Start-Process $setup -ArgumentList '--quiet', '--no-launch', '--no-desktop', '--no-startup' -Wait -PassThru
if ($p.ExitCode -ne 0) { Fail "Installation de la $fromV echouee (code $($p.ExitCode))" }
Wait-Until { (Health).version -eq $fromV } 60 "Rustudio $fromV installe et en marche"
Post 'volume' '{"channel":"Mic","value":77}' | Out-Null
Wait-Until { (Get-Content (Join-Path $data 'state.json') -Raw | ConvertFrom-Json).volumes.Mic -eq 77 } 30 'Reglage enregistre'

try {
  if (-not $Real) {
    $fake = Start-Process python -ArgumentList ('"' + (Join-Path $root 'scripts\fake_github.py') + '"'), ('"' + $serve + '"'), $target, '8767', 'Rustudio.zip', 'Rustudio.sha256' -PassThru -WindowStyle Hidden
    New-ItemProperty $svcKey -Name Environment -PropertyType MultiString -Force -Value @(
      'RUSTUDIO_UPDATE_API=http://127.0.0.1:8767', 'RUSTUDIO_UPDATE_PREFIX=http://127.0.0.1:8767/dl/', 'RUSTUDIO_UPDATE_HOSTS=127.0.0.1') | Out-Null
    Restart-Service Rustudio
    Wait-Until { (Health).version -eq $fromV } 30 "Rustudio $fromV relance sur le faux GitHub"
  }

  # 4. « Mise a jour proposee », puis le clic, avec l'updater de la version de depart
  $t = [Diagnostics.Stopwatch]::StartNew()
  while ($true) {
    try { Post 'update-check' | Out-Null } catch { }
    Start-Sleep 3
    $u = Update
    if ($u.status -eq 'available' -and $u.latest.version -eq $target -and $u.installable) { break }
    if ($u.result -eq 'rate-limited') { Write-Host "::warning::GitHub limite les requetes de la machine de test : test non concluant"; exit 3 }
    if ($t.Elapsed.TotalSeconds -gt 90) { Fail "Rustudio $fromV ne propose pas la $target (statut $($u.status), resultat $($u.result), vue $($u.latest.version), installable $($u.installable) $($u.why_not))" }
  }
  Write-Host ("Rustudio {0} propose la {1} ({2:N0} s)" -f $fromV, $target, $t.Elapsed.TotalSeconds)
  Post 'update-install' | Out-Null
  Wait-Until { (Health).version -eq $target } 300 "Rustudio $target en marche"
  $logf = Join-Path $data 'rustudio.log'
  Wait-Until { Select-String -Path $logf -SimpleMatch "Version $target installée" -Quiet } 60 'Installation terminee'
  if (Select-String -Path $logf -SimpleMatch "Échec, retour à $fromV" -Quiet) { Fail 'Retour arriere pendant la mise a jour' }

  # 5. fichiers, version affichee, sauvegarde de la version de depart, reglages, menage
  if ((Get-Content (Join-Path $dir 'manifest.json') -Raw | ConvertFrom-Json).version -ne $target) { Fail 'Manifeste installe pas a jour' }
  if ((Get-ItemProperty $uninstall).DisplayVersion -ne $target) { Fail 'Version affichee dans Applications pas a jour' }
  if (-not (Get-ChildItem (Join-Path $dir 'backup') -Directory -Filter "*-$fromV" -ErrorAction SilentlyContinue)) { Fail "Pas de sauvegarde de la $fromV" }
  if ((Invoke-RestMethod "$api/api/state").state.volumes.Mic -ne 77) { Fail 'Reglages perdus pendant la mise a jour' }
  if ((Get-Service Rustudio).Status -ne 'Running') { Fail 'Service Rustudio arrete' }
  if (-not (Health).supervised) { Fail 'Le moteur ne tourne plus sous son superviseur' }
  Wait-Until { -not (Get-ScheduledTask -TaskName 'Rustudio Update' -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $data 'update\new')) } 120 'Tache Rustudio Update et copie preparee retirees'
  $u = Update
  if ($u.status -eq 'available') { Fail "La $target propose encore une mise a jour ($($u.latest.version))" }
  Write-Host "Mise a jour $fromV -> $target : OK"
} finally {
  Remove-ItemProperty $svcKey -Name Environment -ErrorAction SilentlyContinue
  if ($fake) { Stop-Process -Id $fake.Id -Force -ErrorAction SilentlyContinue }
}

# 6. desinstallation, pour laisser la machine comme avant
Start-Process (Join-Path $dir 'rustudio-engine.exe') -ArgumentList 'uninstall', '--quiet' -Wait
for ($i = 0; $i -lt 40 -and ((Get-Service Rustudio -ErrorAction SilentlyContinue) -or (Test-Path (Join-Path $dir 'Rustudio.exe'))); $i++) { Start-Sleep 1 }
if (Get-Service Rustudio -ErrorAction SilentlyContinue) { throw 'Service toujours present apres desinstallation' }
Write-Host "Depuis la release $fromV : OK"
