<#
Mesure le démarrage près de l'horloge, avec deux copies de Rustudio.exe et le service déjà lancé.
Exécuter dans une session de test distincte de celle qui utilise Rustudio. Aucun réglage n'est écrit.
#>
param(
    [Parameter(Mandatory)][string]$Before,
    [Parameter(Mandatory)][string]$After,
    [ValidateRange(5, 60)][int]$Seconds = 15
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$session = [Diagnostics.Process]::GetCurrentProcess().SessionId
if (Get-Process Rustudio -ErrorAction SilentlyContinue | Where-Object SessionId -eq $session) {
    throw 'Rustudio est déjà ouvert dans cette session. Utilise une session de test distincte.'
}
$health = Invoke-RestMethod http://127.0.0.1:14565/api/health -TimeoutSec 3
if ($health.status -ne 'ok') { throw 'Le service Rustudio doit être disponible.' }

function WebViews([int]$Root) {
    $ids = [Collections.Generic.HashSet[int]]::new()
    [void]$ids.Add($Root)
    $candidates = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe' AND SessionId=$session")
    do {
        $found = $false
        foreach ($p in $candidates) {
            if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $found = $true }
        }
    } while ($found)
    $ids
}

function Measure-Tray([string]$Label, [string]$Path) {
    $full = (Resolve-Path -LiteralPath $Path).Path
    $p = Start-Process -FilePath $full -ArgumentList '--tray' -WorkingDirectory (Split-Path $full) -PassThru
    try {
        Start-Sleep -Seconds 8
        if ($p.HasExited) { throw "$Label : Rustudio s'est arrêté au démarrage ($($p.ExitCode))." }
        $first = @{}
        foreach ($id in @(WebViews $p.Id)) {
            $sample = Get-Process -Id $id -ErrorAction SilentlyContinue
            if ($sample) { $first[$id] = $sample.CPU }
        }
        $timer = [Diagnostics.Stopwatch]::StartNew()
        Start-Sleep -Seconds $Seconds
        $ids = @(WebViews $p.Id)
        $processes = @(Get-Process -Id $ids -ErrorAction SilentlyContinue)
        $elapsed = $timer.Elapsed.TotalSeconds
        if ($p.HasExited) { throw "$Label : Rustudio s'est arrêté pendant le relevé." }
        $memory = @{}
        foreach ($proc in (Get-CimInstance Win32_PerfFormattedData_PerfProc_Process)) {
            $memory[[int]$proc.IDProcess] = $proc.WorkingSetPrivate
        }
        $cpu = 0.0; $private = 0.0; $resident = 0.0
        foreach ($proc in $processes) {
            if ($first.ContainsKey($proc.Id)) { $cpu += $proc.CPU - $first[$proc.Id] }
            $private += $proc.PrivateMemorySize64
            $resident += $memory[$proc.Id]
        }
        [pscustomobject]@{
            build = $Label
            seconds = [math]::Round($elapsed, 2)
            webview_processes = $processes.Count - 1
            private_resident_mib = [math]::Round($resident / 1MB, 2)
            private_commit_mib = [math]::Round($private / 1MB, 2)
            cpu_one_core_pct = [math]::Round(100 * $cpu / $elapsed, 3)
        }
    } finally {
        if (!$p.HasExited) { Stop-Process -InputObject $p }
        [void]$p.WaitForExit(4000)
    }
}

$results = @(
    Measure-Tray 'before' $Before
    Start-Sleep -Seconds 4
    Measure-Tray 'after' $After
)
$results | ConvertTo-Json -Compress
if ($results[0].webview_processes -eq 0) {
    throw 'Comparaison invalide : l’ancienne version n’a pas lancé WebView2 dans cette session.'
}
