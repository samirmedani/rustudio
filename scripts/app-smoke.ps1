<#
Vérifie le prototype sans installation ni matériel. La fenêtre reste cachée par défaut.
#>
param(
    [Parameter(Mandatory)][string]$EngineExe,
    [Parameter(Mandatory)][string]$MixerExe,
    [ValidateRange(1024, 65535)][int]$Port = 14566,
    [switch]$Visible
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($Port -eq 14565 -or (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)) {
    throw "Le port $Port est réservé ou déjà utilisé."
}
$enginePath = (Resolve-Path -LiteralPath $EngineExe).Path
$mixerPath = (Resolve-Path -LiteralPath $MixerExe).Path
$data = Join-Path $env:TEMP ('rustudio-gpui-' + [Guid]::NewGuid().ToString('N'))
$base = "http://127.0.0.1:$Port"
$engine = $null; $mixer = $null
New-Item -ItemType Directory $data | Out-Null
try {
    $engine = Start-Process $enginePath -ArgumentList "run --child --simulate --port $Port --data-dir `"$data`"" -WindowStyle Hidden -PassThru
    $state = $null
    for ($attempt = 0; $attempt -lt 40 -and -not $state.connected; $attempt++) {
        if ($engine.HasExited) { throw "Le simulateur s'est arrêté ($($engine.ExitCode))." }
        try { $state = Invoke-RestMethod "$base/api/state" -TimeoutSec 1 } catch { }
        Start-Sleep -Milliseconds 125
    }
    if (!$state.sim -or !$state.connected) { throw 'Le moteur simulé ne répond pas.' }

    & $mixerPath --port $Port --self-test
    if ($LASTEXITCODE -ne 0) { throw 'Les commandes du mixeur ont échoué.' }

    $log = Join-Path $data 'mixer.log'
    $errors = Join-Path $data 'mixer-errors.log'
    $args = @('--port', $Port, '--seconds', '8', '--mixer')
    if (!$Visible) { $args += '--hidden' }
    $mixer = Start-Process $mixerPath -ArgumentList $args -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError $errors -PassThru
    # Conserver un handle avant la fin : PowerShell 5 peut perdre ExitCode sans lui.
    [void]$mixer.Handle
    for ($i = 0; $i -lt 18; $i++) {
        if ($mixer.HasExited) { break }
        $payload = @{ index = ($i % 4); value = (($i * 49 + 33) % 255) } | ConvertTo-Json -Compress
        Invoke-RestMethod "$base/api/sim/fader" -Method Post -Headers @{'X-Rustudio' = '1'} -ContentType 'application/json' -Body $payload -TimeoutSec 2 | Out-Null
        Start-Sleep -Milliseconds 250
    }
    if (!$mixer.WaitForExit(15000)) { throw 'Le prototype ne se ferme pas à la fin du relevé.' }
    $text = Get-Content $log -Raw
    if ($mixer.ExitCode -ne 0) {
        $details = Get-Content $errors -Raw
        throw "Prototype arrêté ($($mixer.ExitCode)). $details $text"
    }
    if ($text -notmatch 'snapshots=(\d+) renders=(\d+)' -or [int]$Matches[1] -lt 3) {
        throw "Les déplacements du simulateur ne sont pas suivis : $text"
    }
    $text.Trim()
} finally {
    foreach ($p in @($mixer, $engine)) {
        if ($p -and !$p.HasExited) {
            Stop-Process -InputObject $p
            [void]$p.WaitForExit(4000)
        }
    }
    Remove-Item -LiteralPath $data -Recurse
}
