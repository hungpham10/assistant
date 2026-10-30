[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RemainingArgs
)

$scriptDir = $null
$scriptName = "install.ps1"
if (-not [string]::IsNullOrWhiteSpace($PSCommandPath)) {
    $scriptDir = Split-Path -Parent $PSCommandPath
    $scriptName = Split-Path -Leaf $PSCommandPath
}

if ([string]::IsNullOrWhiteSpace($env:CODEX_GITHUB_REPO)) {
    $env:CODEX_GITHUB_REPO = if ([string]::IsNullOrWhiteSpace($env:OPEN_INTERPRETER_GITHUB_REPO)) {
        "openinterpreter/openinterpreter"
    } else {
        $env:OPEN_INTERPRETER_GITHUB_REPO
    }
}
$env:CODEX_INSTALL_PRODUCT_NAME = if ([string]::IsNullOrWhiteSpace($env:CODEX_INSTALL_PRODUCT_NAME)) {
    "Open Interpreter"
} else {
    $env:CODEX_INSTALL_PRODUCT_NAME
}
$env:CODEX_PACKAGE_ASSET_STEM = if ([string]::IsNullOrWhiteSpace($env:CODEX_PACKAGE_ASSET_STEM)) {
    "open-interpreter-package"
} else {
    $env:CODEX_PACKAGE_ASSET_STEM
}
$env:CODEX_COMMAND_NAME = if ([string]::IsNullOrWhiteSpace($env:CODEX_COMMAND_NAME)) {
    "interpreter"
} else {
    $env:CODEX_COMMAND_NAME
}
$env:CODEX_ALIAS_COMMAND_NAMES = if ([string]::IsNullOrWhiteSpace($env:CODEX_ALIAS_COMMAND_NAMES)) {
    "i"
} else {
    $env:CODEX_ALIAS_COMMAND_NAMES
}
$env:CODEX_RELEASE_TAG_PREFIX = if ([string]::IsNullOrWhiteSpace($env:CODEX_RELEASE_TAG_PREFIX)) {
    "rust-v"
} else {
    $env:CODEX_RELEASE_TAG_PREFIX
}
$env:CODEX_HOME = if ([string]::IsNullOrWhiteSpace($env:INTERPRETER_HOME)) {
    if ([string]::IsNullOrWhiteSpace($env:CODEX_HOME)) {
        Join-Path $env:USERPROFILE ".openinterpreter"
    } else {
        $env:CODEX_HOME
    }
} else {
    $env:INTERPRETER_HOME
}
$env:CODEX_INSTALL_DIR = if ([string]::IsNullOrWhiteSpace($env:OPEN_INTERPRETER_INSTALL_DIR)) {
    if ([string]::IsNullOrWhiteSpace($env:CODEX_INSTALL_DIR)) {
        Join-Path $env:LOCALAPPDATA "Programs\Open Interpreter\bin"
    } else {
        $env:CODEX_INSTALL_DIR
    }
} else {
    $env:OPEN_INTERPRETER_INSTALL_DIR
}
$env:CODEX_RELEASE = if ([string]::IsNullOrWhiteSpace($env:OPEN_INTERPRETER_RELEASE)) {
    if ([string]::IsNullOrWhiteSpace($env:CODEX_RELEASE)) {
        "latest"
    } else {
        $env:CODEX_RELEASE
    }
} else {
    $env:OPEN_INTERPRETER_RELEASE
}
if (-not [string]::IsNullOrWhiteSpace($env:OPEN_INTERPRETER_NONINTERACTIVE)) {
    $env:CODEX_NON_INTERACTIVE = $env:OPEN_INTERPRETER_NONINTERACTIVE
}

if (-not [string]::IsNullOrWhiteSpace($scriptDir)) {
    $siblingInstaller = Join-Path $scriptDir "install.ps1"
    if ($scriptName -ne "install.ps1" -and (Test-Path -LiteralPath $siblingInstaller -PathType Leaf)) {
        & $siblingInstaller @RemainingArgs
        exit $LASTEXITCODE
    }
}

$installerText = Invoke-RestMethod -Uri "https://www.openinterpreter.com/install.ps1"
$installer = [scriptblock]::Create($installerText)
& $installer @RemainingArgs
