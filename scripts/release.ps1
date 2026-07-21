[CmdletBinding()]
param(
    [string[]] $PythonVersion,
    [string] $Msys2Bin = "C:\msys64\ucrt64\bin",
    [string] $OutDir = "dist",
    [string] $PythonInstallDir = ".release-python",
    [switch] $SkipTests,
    [switch] $Publish,
    [ValidateSet("pypi", "testpypi")]
    [string] $Repository = "pypi"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($null -eq $PythonVersion) {
    $PythonVersion = @()
} else {
    $PythonVersion = @(
        $PythonVersion |
            ForEach-Object { $_ -split ',' } |
            ForEach-Object { $_.Trim() } |
            Where-Object { $_ }
    )
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

if (-not (Test-Path -LiteralPath $Msys2Bin -PathType Container)) {
    throw "MSYS2 directory not found: $Msys2Bin"
}
$env:Path = "$Msys2Bin;$env:Path"

$installed = @{}
if (Get-Command py -ErrorAction SilentlyContinue) {
    $launcherOutput = (& py -0p 2>&1) -join "`n"
    foreach ($line in $launcherOutput -split "`r?`n") {
        if ($line -match '-V:(3\.\d+)(?:-\d+)?\s+\*?\s*(.+?)\s*$') {
            $version = $Matches[1]
            $path = $Matches[2].Trim()
            $parts = $version.Split('.')
            if ([int]$parts[0] -eq 3 -and [int]$parts[1] -ge 10 -and -not $installed.ContainsKey($version)) {
                $installed[$version] = $path
            }
        }
    }
}

if ($PythonVersion.Count -eq 0) {
    $PythonVersion = @($installed.Keys | Sort-Object { [version]$_ })
}
if ($PythonVersion.Count -eq 0) {
    throw "No CPython 3.10 or newer installation was found. Install one bootstrap Python or pass -PythonVersion after installing one."
}

foreach ($version in $PythonVersion) {
    if ($version -notmatch '^3\.\d+$' -or [version]$version -lt [version]'3.10') {
        throw "Unsupported Python version '$version'. Use minor versions such as 3.10."
    }
}

$toolVenv = Join-Path $repoRoot ".release-venv"
$toolPython = Join-Path $toolVenv "Scripts\python.exe"
if (-not (Test-Path -LiteralPath $toolPython)) {
    if ($installed.Count -eq 0) {
        throw "At least one system CPython 3.10+ installation is required to bootstrap the release tools."
    }
    $bootstrapVersion = @($installed.Keys | Sort-Object { [version]$_ })[-1]
    & $installed[$bootstrapVersion] -m venv $toolVenv
    if ($LASTEXITCODE -ne 0) { throw "Failed to create release environment." }
}
& $toolPython -m pip install --upgrade "maturin>=1.1,<2.0" "twine>=5,<7" "uv>=0.8,<1"
if ($LASTEXITCODE -ne 0) { throw "Failed to install release tools." }

$toolUv = Join-Path $toolVenv "Scripts\uv.exe"
if ([System.IO.Path]::IsPathRooted($PythonInstallDir)) {
    $resolvedPythonInstallDir = $PythonInstallDir
} else {
    $resolvedPythonInstallDir = Join-Path $repoRoot $PythonInstallDir
}
$resolvedPythonInstallDir = [System.IO.Path]::GetFullPath($resolvedPythonInstallDir)
$env:UV_PYTHON_INSTALL_DIR = $resolvedPythonInstallDir

$interpreters = foreach ($version in $PythonVersion) {
    if ($installed.ContainsKey($version)) {
        $python = $installed[$version]
    } else {
        Write-Host "Installing managed CPython $version into $resolvedPythonInstallDir..."
        & $toolUv python install $version --install-dir $resolvedPythonInstallDir --no-bin --no-registry
        if ($LASTEXITCODE -ne 0) { throw "Failed to install managed Python $version." }
        $python = (& $toolUv python find $version --managed-python --no-python-downloads --no-project).Trim()
        if ($LASTEXITCODE -ne 0 -or -not $python) { throw "Could not locate managed Python $version after installation." }
    }
    & $python -c "import sys; assert sys.version_info[:2] == tuple(map(int, '$version'.split('.')))"
    if ($LASTEXITCODE -ne 0) { throw "Could not run Python $version at $python" }
    [pscustomobject]@{ Version = $version; Path = $python }
}

$resolvedOutDir = Join-Path $repoRoot $OutDir
New-Item -ItemType Directory -Force -Path $resolvedOutDir | Out-Null

foreach ($interpreter in $interpreters) {
    Write-Host "Building CPython $($interpreter.Version) wheel..."
    & $toolPython -m maturin build --release --locked --interpreter $interpreter.Path --out $resolvedOutDir
    if ($LASTEXITCODE -ne 0) { throw "Wheel build failed for Python $($interpreter.Version)." }
}

$wheels = @(
    foreach ($interpreter in $interpreters) {
        $tag = $interpreter.Version.Replace('.', '')
        $matchingWheels = @(Get-ChildItem -LiteralPath $resolvedOutDir -Filter "*.whl" | Where-Object Name -Match "-cp$tag-cp$tag-")
        if ($matchingWheels.Count -ne 1) {
            throw "Expected exactly one newly built cp$tag wheel in $resolvedOutDir; found $($matchingWheels.Count)."
        }
        $matchingWheels[0]
    }
)
& $toolPython -m twine check @($wheels.FullName)
if ($LASTEXITCODE -ne 0) { throw "Twine package validation failed." }

if (-not $SkipTests) {
    foreach ($interpreter in $interpreters) {
        $tag = $interpreter.Version.Replace('.', '')
        $wheel = @($wheels | Where-Object Name -Match "-cp$tag-cp$tag-")
        if ($wheel.Count -ne 1) {
            throw "Expected exactly one cp$tag wheel in $resolvedOutDir; found $($wheel.Count). Clean dist or use a separate -OutDir."
        }
        $testVenv = Join-Path ([System.IO.Path]::GetTempPath()) "nifpga-fast-fifo-recv-cp$tag-$PID"
        try {
            & $interpreter.Path -m venv $testVenv
            if ($LASTEXITCODE -ne 0) { throw "Failed to create test environment for Python $($interpreter.Version)." }
            $testPython = Join-Path $testVenv "Scripts\python.exe"
            & $testPython -m pip install --disable-pip-version-check $wheel[0].FullName
            if ($LASTEXITCODE -ne 0) { throw "Failed to install the Python $($interpreter.Version) wheel." }
            & $testPython -c "import nifpga_fast_fifo_recv; print(nifpga_fast_fifo_recv.__file__)"
            if ($LASTEXITCODE -ne 0) { throw "Import test failed for Python $($interpreter.Version)." }
        }
        finally {
            if (Test-Path -LiteralPath $testVenv) { Remove-Item -LiteralPath $testVenv -Recurse -Force }
        }
    }
}

Write-Host "Validated wheels:"
$wheels.FullName | ForEach-Object { Write-Host "  $_" }

if ($Publish) {
    Write-Host "Uploading to $Repository..."
    & $toolPython -m twine upload --repository $Repository @($wheels.FullName)
    if ($LASTEXITCODE -ne 0) { throw "Upload to $Repository failed." }
} else {
    Write-Host "Upload skipped. Re-run with -Publish (and optionally -Repository testpypi) when ready."
}
