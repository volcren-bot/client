# Installs, updates and uninstalls the MSI in both modes on a real Windows runner, updating the way the loader does.
param(
    [Parameter(Mandatory)] [string] $OldMsi,
    [Parameter(Mandatory)] [string] $NewMsi,
    [Parameter(Mandatory)] [string] $OldVersion,
    [Parameter(Mandatory)] [string] $NewVersion
)

$failed = 0
$uninstall = "Software\Microsoft\Windows\CurrentVersion\Uninstall"
$work = Join-Path $env:RUNNER_TEMP "msi-test"
New-Item -ItemType Directory -Force $work | Out-Null

function Msi([string] $arguments) {
    $log = Join-Path $work "$([guid]::NewGuid()).log"
    $process = Start-Process msiexec -ArgumentList "$arguments /norestart /l*v `"$log`"" -Wait -PassThru
    Write-Host "msiexec $arguments -> $($process.ExitCode)"
    if ($process.ExitCode -ne 0) {
        Get-Content $log -Tail 60 | Write-Host
    }
}

function Entries([string] $hive) {
    @(Get-ChildItem "${hive}:\$uninstall" -ErrorAction SilentlyContinue |
        Get-ItemProperty | Where-Object { $_.DisplayName -eq "VOLCREN" })
}

function Check([string] $name, [string] $hive, [string] $otherHive, [string] $folder, [string] $version) {
    $entries = Entries $hive
    $others = Entries $otherHive
    $problems = @()
    if ($others.Count -ne 0) { $problems += "$($others.Count) entries in $otherHive" }
    if ($version) {
        if ($entries.Count -ne 1) { $problems += "$($entries.Count) entries in $hive" }
        elseif ($entries[0].DisplayVersion -ne $version) { $problems += "version $($entries[0].DisplayVersion)" }
        elseif ($entries[0].InstallLocation.TrimEnd("\") -ne $folder) { $problems += "location $($entries[0].InstallLocation)" }
        foreach ($file in "loader.exe", "injector.exe", "agent.dll") {
            if (-not (Test-Path (Join-Path $folder $file))) { $problems += "no $file" }
        }
    } else {
        if ($entries.Count -ne 0) { $problems += "$($entries.Count) entries left in $hive" }
        if (Test-Path (Join-Path $folder "loader.exe")) { $problems += "files left in $folder" }
    }
    if ($problems.Count -eq 0) {
        Write-Host "PASS $name"
    } else {
        Write-Host "FAIL $name : $($problems -join ', ')"
        $script:failed++
    }
}

# What update::apply starts: a copy of the installed loader, outside the install, for a process that has already exited.
function Update-Like-The-Loader([string] $folder, [string] $scope) {
    $helper = Join-Path $work "finish-update.exe"
    Copy-Item (Join-Path $folder "loader.exe") $helper -Force
    $log = Join-Path $work "update-$scope.log"
    $exe = Join-Path $folder "loader.exe"
    $arguments = "--finish-update `"$NewMsi`" 4294967295 `"$exe`" $scope `"$log`""
    $process = Start-Process $helper -ArgumentList $arguments -PassThru
    if (-not $process.WaitForExit(300000)) {
        Write-Host "FAIL the update helper is still running"
        $script:failed++
        Stop-Process $process -Force
    }
    # The update starts the installed loader, which can't do much on a runner.
    Start-Sleep -Seconds 3
    $started = @(Get-Process loader -ErrorAction SilentlyContinue)
    Write-Host "Loaders running after the update: $($started.Count)"
    $started | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    if (Test-Path $log) {
        Select-String -Path $log -Pattern "MainEngineThread is returning" | ForEach-Object { Write-Host $_.Line }
    } else {
        Write-Host "No installer log at $log"
    }
}

$perUserFolder = Join-Path $env:LOCALAPPDATA "Programs\VOLCREN"
$perMachineFolder = Join-Path $env:ProgramFiles "VOLCREN"

# With no choice made the default is per-user, even for an administrator.
Msi "/i `"$OldMsi`" /qn"
Check "default install is per-user" HKCU HKLM $perUserFolder $OldVersion
Update-Like-The-Loader $perUserFolder "user"
Check "per-user update" HKCU HKLM $perUserFolder $NewVersion
Msi "/x `"$NewMsi`" /qn"
Check "per-user uninstall" HKCU HKLM $perUserFolder ""

Msi "/i `"$OldMsi`" /qn ALLUSERS=1"
Check "per-machine install" HKLM HKCU $perMachineFolder $OldVersion
Update-Like-The-Loader $perMachineFolder "machine"
Check "per-machine update" HKLM HKCU $perMachineFolder $NewVersion
Msi "/x `"$NewMsi`" /qn"
Check "per-machine uninstall" HKLM HKCU $perMachineFolder ""

exit $failed
