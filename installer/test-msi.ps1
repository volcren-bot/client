param(
    [Parameter(Mandatory)] [string] $OldMsi,
    [Parameter(Mandatory)] [string] $NewMsi,
    [Parameter(Mandatory)] [string] $OldVersion,
    [Parameter(Mandatory)] [string] $NewVersion
)

$failed = 0
$work = Join-Path $env:RUNNER_TEMP "msi-test"
New-Item -ItemType Directory -Force $work | Out-Null

$installer = New-Object -ComObject WindowsInstaller.Installer
$upgradeCode = "{DF7C7129-38EF-4E32-B9EE-F6FCFA75ED84}"

function Show-Context([string] $log) {
    Select-String -Path $log -Pattern "Property\(S\): (ALLUSERS|MSIINSTALLPERUSER|APPLICATIONFOLDER|Privileged|VolcrenPerUserFolder) = " |
        ForEach-Object { Write-Host "  $($_.Line)" }
}

function Msi([string] $arguments) {
    $log = Join-Path $work "$([guid]::NewGuid()).log"
    $process = Start-Process msiexec -ArgumentList "$arguments /norestart /l*v `"$log`"" -Wait -PassThru
    Write-Host "msiexec $arguments -> $($process.ExitCode)"
    Show-Context $log
    if ($process.ExitCode -ne 0) {
        Get-Content $log -Tail 60 | Write-Host
    }
}

function Products {
    foreach ($product in $installer.RelatedProducts($upgradeCode)) {
        [pscustomobject]@{
            Assignment = $installer.ProductInfo($product, "AssignmentType")
            Location   = $installer.ProductInfo($product, "InstallLocation")
            Version    = $installer.ProductInfo($product, "VersionString")
        }
    }
}

function Check([string] $name, [string] $assignment, [string] $folder, [string] $version) {
    $products = @(Products)
    $problems = @()
    if ($version) {
        if ($products.Count -ne 1) { $problems += "$($products.Count) products" }
        else {
            if ($products[0].Assignment -ne $assignment) { $problems += "assignment $($products[0].Assignment)" }
            if ($products[0].Version -ne $version) { $problems += "version $($products[0].Version)" }
            if ($products[0].Location.TrimEnd("\") -ne $folder) { $problems += "location $($products[0].Location)" }
        }
        foreach ($file in "loader.exe", "injector.exe", "agent.dll", "luau-analyze.exe") {
            if (-not (Test-Path (Join-Path $folder $file))) { $problems += "no $file" }
        }
    } else {
        if ($products.Count -ne 0) { $problems += "$($products.Count) products left" }
        if (Test-Path (Join-Path $folder "loader.exe")) { $problems += "files left in $folder" }
    }
    if ($problems.Count -eq 0) {
        Write-Host "PASS $name"
    } else {
        Write-Host "FAIL $name : $($problems -join ', ')"
        $script:failed++
    }
}

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
    Start-Sleep -Seconds 3
    $started = @(Get-Process loader -ErrorAction SilentlyContinue)
    Write-Host "Loaders running after the update: $($started.Count)"
    $started | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
    if (Test-Path $log) {
        Show-Context $log
        Select-String -Path $log -Pattern "MainEngineThread is returning" | ForEach-Object { Write-Host $_.Line }
    } else {
        Write-Host "No installer log at $log"
    }
}

$perUserFolder = Join-Path $env:LOCALAPPDATA "Programs\VOLCREN"
$perMachineFolder = Join-Path $env:ProgramFiles "VOLCREN"

Msi "/i `"$OldMsi`" /qn"
Check "default install is per-user" "0" $perUserFolder $OldVersion
Update-Like-The-Loader $perUserFolder "user"
Check "per-user update" "0" $perUserFolder $NewVersion
Msi "/x `"$NewMsi`" /qn"
Check "per-user uninstall" "" $perUserFolder ""

Msi "/i `"$OldMsi`" /qn ALLUSERS=1"
Check "per-machine install" "1" $perMachineFolder $OldVersion
Update-Like-The-Loader $perMachineFolder "machine"
Check "per-machine update" "1" $perMachineFolder $NewVersion
Msi "/x `"$NewMsi`" /qn"
Check "per-machine uninstall" "" $perMachineFolder ""

exit $failed
