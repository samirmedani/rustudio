# Banc de la CI Windows, sur buffers mémoire et registre de test.
$ErrorActionPreference = 'Stop'
cargo build --locked --release -p rustudio-micro-apo --example buffer_host
if ($LASTEXITCODE) { throw "Compilation de l’hôte COM échouée." }
cargo build --locked --release -p rustudio-windows-mic --example registry_check
if ($LASTEXITCODE) { throw "Compilation du banc de registre échouée." }
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$hostExe = (Resolve-Path "$target/release/examples/buffer_host.exe").Path
$dll = (Resolve-Path "$target/release/rustudio_micro_apo.dll").Path
& $hostExe $dll
if ($LASTEXITCODE) { throw "Le traitement des buffers COM a échoué." }

# Le propriétaire de la mémoire partagée est vérifié par la DLL. Les deux
# contrôles suivants tournent avec les mêmes privilèges que le service moteur.
function Test-AsSystem([string]$title, $action) {
    $name = 'Rustudio-Micro-Test-' + [guid]::NewGuid().ToString('N')
    Register-ScheduledTask -TaskName $name -Action $action -User SYSTEM -RunLevel Highest | Out-Null
    try {
        Start-ScheduledTask -TaskName $name
        $end = (Get-Date).AddSeconds(90)
        do {
            Start-Sleep -Milliseconds 200
            $info = Get-ScheduledTaskInfo -TaskName $name
            $state = (Get-ScheduledTask -TaskName $name).State
            if ($info.LastRunTime.Year -gt 2000 -and $state -eq 'Ready') {
                if ($info.LastTaskResult -ne 0) { throw "$title (code $($info.LastTaskResult))." }
                Write-Host "$title OK"
                return
            }
        } while ((Get-Date) -lt $end)
        throw "$title a dépassé le délai prévu."
    } finally {
        Unregister-ScheduledTask -TaskName $name -Confirm:$false
    }
}
Test-AsSystem 'Gain, latence et vumètres par mémoire partagée' (New-ScheduledTaskAction -Execute $hostExe -Argument ('"' + $dll + '" --controlled'))
$registry = (Resolve-Path "$target/release/examples/registry_check.exe").Path
Test-AsSystem 'Migration et restauration dans une ruche isolée' (New-ScheduledTaskAction -Execute $registry)
