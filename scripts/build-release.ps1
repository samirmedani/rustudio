<#
Fabrique la release de Rustudio (sous Windows, Rust stable) :
  dist\Rustudio-Setup.exe   installeur en un seul fichier (l'appli est embarquee)
  dist\Rustudio.zip         archive de la mise a jour integree, et dist\Rustudio.sha256
  dist\GoXLR-Mini.zip       passage depuis GoXLR Mini 4.x (publie aussi sur l'ancien depot), et dist\GoXLR-Mini.sha256
Le manifeste (manifest.json) donne la version, le commit et l'empreinte de chaque fichier.
-PayloadOnly : seulement Rustudio.zip et Rustudio.sha256 (test de la mise a jour integree).
#>
param([string]$Out = (Join-Path $PSScriptRoot '..\dist'), [switch]$PayloadOnly)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$utf8 = New-Object Text.UTF8Encoding $false
$nl = [string][char]10
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem

function New-Zip([string]$src, [string]$zip) {
  if (Test-Path $zip) { Remove-Item $zip -Force }
  $fs = [IO.File]::Open($zip, [IO.FileMode]::Create)
  $za = New-Object IO.Compression.ZipArchive($fs, [IO.Compression.ZipArchiveMode]::Create)
  Get-ChildItem $src -Recurse -File | Sort-Object FullName | ForEach-Object {
    $rel = $_.FullName.Substring($src.Length + 1).Replace('\', '/')
    [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($za, $_.FullName, $rel, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
  }
  $za.Dispose(); $fs.Dispose()
}
function Write-Manifest([string]$dir, [string]$name, [string]$version, [string]$commit) {
  $files = [ordered]@{}
  Get-ChildItem $dir -Recurse -File | Sort-Object FullName | ForEach-Object {
    $files[$_.FullName.Substring($dir.Length + 1).Replace('\', '/')] = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
  }
  $files['manifest.json'] = ''
  $man = [ordered]@{ name = $name; version = $version; release = $true; commit = $commit; built = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); files = $files }
  [IO.File]::WriteAllText((Join-Path $dir 'manifest.json'), ($man | ConvertTo-Json -Depth 5), $utf8)
}
function Fresh([string]$dir) { if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }; New-Item -ItemType Directory -Force $dir | Out-Null; (Resolve-Path $dir).Path }
function Sha([string]$f) { (Get-FileHash $f -Algorithm SHA256).Hash.ToLower() }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$commit = ''
try { $commit = [string](git -C $root rev-parse --short HEAD 2>$null) } catch { }
Write-Host "Rustudio $version ($commit)"
Push-Location $root
try {
  cargo build --release --locked -p rustudio-engine -p rustudio-app
  if ($LASTEXITCODE -ne 0) { throw 'Compilation du moteur ou de l appli echouee' }
  $bin = Join-Path $root 'target\release'

  # 1. archive de l'appli (mise a jour integree)
  $stage = Fresh (Join-Path $Out 'payload')
  Copy-Item (Join-Path $bin 'rustudio-engine.exe') $stage
  Copy-Item (Join-Path $bin 'Rustudio.exe') $stage
  Copy-Item (Join-Path $root 'LICENSE') (Join-Path $stage 'LICENSE.txt')
  Copy-Item (Join-Path $root 'THIRD-PARTY-NOTICES.md') $stage
  Write-Manifest $stage 'Rustudio' $version $commit
  $zip = Join-Path $Out 'Rustudio.zip'
  New-Zip $stage $zip
  if ($PayloadOnly) {
    [IO.File]::WriteAllText((Join-Path $Out 'Rustudio.sha256'), ((Sha $zip) + '  Rustudio.zip' + $nl), $utf8)
    Write-Host ("Rustudio.zip : {0:N1} Mo" -f ((Get-Item $zip).Length / 1MB))
    return
  }

  # 2. installeur : l'archive est embarquee dans l'exe
  $env:RUSTUDIO_PAYLOAD = $zip
  cargo build --release --locked -p rustudio-setup
  if ($LASTEXITCODE -ne 0) { throw 'Compilation de l installeur echouee' }
  Remove-Item Env:RUSTUDIO_PAYLOAD
  $exe = Join-Path $Out 'Rustudio-Setup.exe'
  Copy-Item (Join-Path $bin 'Rustudio-Setup.exe') $exe -Force

  # 3. passage depuis GoXLR Mini 4.x : son updater attend un Python et goxlrctl.py ; ce goxlrctl.py lance l'installeur
  $br = Fresh (Join-Path $Out 'bridge')
  $pyUrl = 'https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip'
  $pySha = '009d6bf7e3b2ddca3d784fa09f90fe54336d5b60f0e0f305c37f400bf83cfd3b'
  $pyZip = Join-Path $Out 'python-3.11.9-embed-amd64.zip'
  if (-not (Test-Path $pyZip) -or (Sha $pyZip) -ne $pySha) { Invoke-WebRequest $pyUrl -OutFile $pyZip -UseBasicParsing }
  if ((Sha $pyZip) -ne $pySha) { throw 'Empreinte du Python embarque incorrecte' }
  Expand-Archive $pyZip -DestinationPath (Join-Path $br 'python') -Force
  Copy-Item (Join-Path $root 'legacy\bridge\goxlrctl.py') $br
  Copy-Item $exe (Join-Path $br 'Rustudio-Setup.exe')
  [IO.File]::WriteAllText((Join-Path $br 'ui.html'), '<!doctype html><meta charset="utf-8"><title>Rustudio</title><p>GoXLR Mini est devenu Rustudio.</p>', $utf8)
  New-Item -ItemType Directory -Force (Join-Path $br 'app') | Out-Null
  [IO.File]::WriteAllText((Join-Path $br 'app\GoXLR Mini.exe'), 'Remplace par Rustudio.', $utf8)   # attendu par l'updater 4.x, jamais lance
  Write-Manifest $br 'GoXLR Mini' $version $commit
  $bridge = Join-Path $Out 'GoXLR-Mini.zip'
  New-Zip $br $bridge

  # 4. empreintes (la mise a jour integree lit la premiere ligne : l'archive reste en tete)
  [IO.File]::WriteAllText((Join-Path $Out 'Rustudio.sha256'), ((Sha $zip) + '  Rustudio.zip' + $nl + (Sha $exe) + '  Rustudio-Setup.exe' + $nl), $utf8)
  [IO.File]::WriteAllText((Join-Path $Out 'GoXLR-Mini.sha256'), ((Sha $bridge) + '  GoXLR-Mini.zip' + $nl), $utf8)
  foreach ($f in $exe, $zip, $bridge) { Write-Host ("{0} : {1:N1} Mo" -f (Split-Path $f -Leaf), ((Get-Item $f).Length / 1MB)) }
} finally { Pop-Location }
