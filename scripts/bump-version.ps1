<#
.SYNOPSIS
    Bumps TableMinus workspace crate versions, updates Cargo.lock and CHANGELOG.md,
    and optionally creates a release commit, annotated tag, and git push.

.EXAMPLE
    .\scripts\bump-version.ps1 -Bump patch -DryRun

.EXAMPLE
    .\scripts\bump-version.ps1 -Version 0.2.0 -Tag

.EXAMPLE
    .\scripts\bump-version.ps1 -Bump minor -Tag -Push
#>
[CmdletBinding(DefaultParameterSetName = 'Bump')]
param(
    [Parameter(ParameterSetName = 'Version', Mandatory = $true)]
    [string]$Version,

    [Parameter(ParameterSetName = 'Bump', Mandatory = $true)]
    [ValidateSet('patch', 'minor', 'major')]
    [string]$Bump,

    [switch]$Tag,
    [switch]$Push,
    [switch]$DryRun,
    [switch]$AllowDirty
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoUrl = 'https://github.com/williamcachamwri/TableMinus'
$SemVerRegex = '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$'

function Write-Utf8NoBom {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Content
    )
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Content, $utf8NoBom)
}

function Get-PackageVersion {
    param([Parameter(Mandatory = $true)][string]$CargoTomlPath)
    $lines = Get-Content -Path $CargoTomlPath -Encoding UTF8
    $inPackage = $false
    foreach ($line in $lines) {
        if ($line -match '^\s*\[([^\]]+)\]\s*$') {
            $inPackage = ($Matches[1].Trim() -eq 'package')
            continue
        }
        if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"\s*$') {
            return $Matches[1]
        }
    }
    throw "Could not find [package] version in '$CargoTomlPath'."
}

function Update-PackageVersionInContent {
    param(
        [Parameter(Mandatory = $true)][string]$RawText,
        [Parameter(Mandatory = $true)][string]$NewVersion
    )
    $newline = if ($RawText.Contains("`r`n")) { "`r`n" } else { "`n" }
    $lines = $RawText -split "\r?\n", -1
    $inPackage = $false
    $replaced = $false

    for ($i = 0; $i -lt $lines.Length; $i++) {
        $line = $lines[$i]
        if ($line -match '^\s*\[([^\]]+)\]\s*$') {
            $inPackage = ($Matches[1].Trim() -eq 'package')
            continue
        }
        if ($inPackage -and -not $replaced -and $line -match '^(\s*version\s*=\s*")([^"]+)(".*)$') {
            $lines[$i] = "$($Matches[1])$NewVersion$($Matches[3])"
            $replaced = $true
        }
    }

    if (-not $replaced) {
        throw 'Failed to locate [package] version line for replacement.'
    }

    return ($lines -join $newline)
}

function Get-LatestVersionTag {
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $tags = @(git tag --list 'v[0-9]*' --sort=-v:refname 2>$null)
        if ($LASTEXITCODE -eq 0 -and $tags.Count -gt 0) {
            return "$($tags[0])".Trim()
        }
        return ''
    } finally {
        $ErrorActionPreference = $prevEap
    }
}

function Get-FallbackChangelogBody {
    param([string]$PreviousTag)

    $range = if ([string]::IsNullOrWhiteSpace($PreviousTag)) { 'HEAD' } else { "$PreviousTag..HEAD" }
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $subjects = @(git log $range --no-merges --pretty=format:%s 2>$null)
        if ($LASTEXITCODE -ne 0) {
            $subjects = @()
        }
    } finally {
        $ErrorActionPreference = $prevEap
    }

    $groups = [ordered]@{
        'Added'         = [System.Collections.Generic.List[string]]::new()
        'Changed'       = [System.Collections.Generic.List[string]]::new()
        'Fixed'         = [System.Collections.Generic.List[string]]::new()
        'Documentation' = [System.Collections.Generic.List[string]]::new()
        'CI & Build'    = [System.Collections.Generic.List[string]]::new()
        'Maintenance'   = [System.Collections.Generic.List[string]]::new()
    }

    foreach ($subject in $subjects) {
        if ([string]::IsNullOrWhiteSpace($subject)) {
            continue
        }
        if ($subject -match '^chore\(release\):') {
            continue
        }

        if ($subject -match '^(?<type>[a-zA-Z]+)(?:\((?<scope>[^)]+)\))?(?<breaking>!)?:\s*(?<desc>.+)$') {
            $type = $Matches['type'].ToLowerInvariant()
            $scope = $Matches['scope']
            $breaking = $Matches['breaking']
            $desc = $Matches['desc'].Trim()
            if ($desc.Length -gt 0) {
                $desc = $desc.Substring(0, 1).ToUpperInvariant() + $desc.Substring(1)
            }

            $prefix = ''
            if (-not [string]::IsNullOrWhiteSpace($scope)) {
                $prefix += "**${scope}**: "
            }
            if ($breaking -eq '!') {
                $prefix += '[**breaking**] '
            }
            $entry = "- $prefix$desc"

            if ($scope -in @('i18n', 'l10n') -or $type -eq 'i18n') {
                $groups['Changed'].Add($entry)
                continue
            }

            switch ($type) {
                'feat'     { $groups['Added'].Add($entry) }
                'fix'      { $groups['Fixed'].Add($entry) }
                'perf'     { $groups['Changed'].Add($entry) }
                'refactor' { $groups['Changed'].Add($entry) }
                'style'    { $groups['Changed'].Add($entry) }
                'docs'     { $groups['Documentation'].Add($entry) }
                'ci'       { $groups['CI & Build'].Add($entry) }
                'build'    { $groups['CI & Build'].Add($entry) }
                'chore'    { $groups['Maintenance'].Add($entry) }
                'test'     { $groups['Maintenance'].Add($entry) }
                default    { $groups['Changed'].Add($entry) }
            }
        }
    }

    $sb = [System.Text.StringBuilder]::new()
    foreach ($section in $groups.Keys) {
        $items = $groups[$section]
        if ($items.Count -eq 0) {
            continue
        }
        [void]$sb.AppendLine("### $section")
        [void]$sb.AppendLine()
        foreach ($item in $items) {
            [void]$sb.AppendLine($item)
        }
        [void]$sb.AppendLine()
    }

    $result = $sb.ToString().Trim()
    if ([string]::IsNullOrWhiteSpace($result)) {
        return "### Maintenance`n`n- Maintenance release."
    }
    return $result
}

function Get-ReleaseSection {
    param(
        [Parameter(Mandatory = $true)][string]$NewVersion,
        [Parameter(Mandatory = $true)][string]$ReleaseDate,
        [string]$PreviousTag
    )

    $cliffCmd = Get-Command git-cliff -ErrorAction SilentlyContinue
    if ($null -ne $cliffCmd -and (Test-Path 'cliff.toml')) {
        $cliffOutput = (& git-cliff --config cliff.toml --unreleased --tag "v$NewVersion" --strip all 2>$null)
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace(($cliffOutput -join "`n"))) {
            $body = ($cliffOutput -join "`n").Trim()
            return "## [$NewVersion] - $ReleaseDate`n`n$body"
        }
    }

    $fallbackBody = Get-FallbackChangelogBody -PreviousTag $PreviousTag
    return "## [$NewVersion] - $ReleaseDate`n`n$fallbackBody"
}

function Update-ChangelogContent {
    param(
        [Parameter(Mandatory = $true)][string]$RawChangelog,
        [Parameter(Mandatory = $true)][string]$NewVersion,
        [Parameter(Mandatory = $true)][string]$NewSection
    )

    $newline = if ($RawChangelog.Contains("`r`n")) { "`r`n" } else { "`n" }
    $normalized = $RawChangelog -replace "`r`n", "`n"

    $unreleasedPattern = '(?m)^## \[Unreleased\]\s*$'
    if ($normalized -notmatch $unreleasedPattern) {
        throw "CHANGELOG.md must contain an '## [Unreleased]' heading."
    }

    $normalizedSection = ($NewSection -replace "`r`n", "`n").Trim()
    $updated = [regex]::Replace(
        $normalized,
        $unreleasedPattern,
        "## [Unreleased]`n`n$normalizedSection",
        1
    )

    $unreleasedLinkPattern = '(?m)^\[Unreleased\]:\s*.+$'
    $newUnreleasedLink = "[Unreleased]: $RepoUrl/compare/v$NewVersion...HEAD"
    $newVersionLink = "[$NewVersion]: $RepoUrl/releases/tag/v$NewVersion"

    $prevTagMatch = [regex]::Match($normalized, '(?m)^\[([^\]]+)\]:\s*\S+/releases/tag/(v[^\s]+)')
    if ($prevTagMatch.Success) {
        $prevTag = $prevTagMatch.Groups[2].Value
        $newVersionLink = "[$NewVersion]: $RepoUrl/compare/$prevTag...v$NewVersion"
    } else {
        $prevCompareMatch = [regex]::Match($normalized, '(?m)^\[Unreleased\]:\s*\S+/compare/(v[^\.\s]+)\.\.\.HEAD')
        if ($prevCompareMatch.Success) {
            $prevTag = $prevCompareMatch.Groups[1].Value
            $newVersionLink = "[$NewVersion]: $RepoUrl/compare/$prevTag...v$NewVersion"
        }
    }

    if ($updated -match $unreleasedLinkPattern) {
        $updated = [regex]::Replace(
            $updated,
            $unreleasedLinkPattern,
            "$newUnreleasedLink`n$newVersionLink",
            1
        )
    } else {
        $updated = $updated.TrimEnd() + "`n`n$newUnreleasedLink`n$newVersionLink`n"
    }

    if ($newline -eq "`r`n") {
        $updated = $updated -replace "`n", "`r`n"
    }
    return $updated
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    $dirtyFiles = @(git status --porcelain)
    if ($LASTEXITCODE -ne 0) {
        throw 'Failed to query git status.'
    }
    if ($dirtyFiles.Count -gt 0 -and -not $AllowDirty) {
        if ($DryRun) {
            Write-Warning "Working tree has $($dirtyFiles.Count) uncommitted change(s); continuing because -DryRun is active."
        } else {
            throw 'Working tree is not clean. Commit or stash changes first, or pass -AllowDirty.'
        }
    }

    $appCargoPath = Join-Path $repoRoot 'crates\tableminus-windows-app\Cargo.toml'
    $currentVersion = Get-PackageVersion -CargoTomlPath $appCargoPath
    if ($currentVersion -notmatch $SemVerRegex) {
        throw "Current version '$currentVersion' in crates/tableminus-windows-app/Cargo.toml is not valid SemVer."
    }

    if ($PSCmdlet.ParameterSetName -eq 'Version') {
        $candidate = $Version.Trim().TrimStart('v')
        if ($candidate -notmatch $SemVerRegex) {
            throw "Target version '$Version' is not valid SemVer (expected X.Y.Z or X.Y.Z-prerelease)."
        }
        $newVersion = $candidate
    } else {
        if ($currentVersion -notmatch '^(\d+)\.(\d+)\.(\d+)') {
            throw "Cannot bump non-numeric version '$currentVersion'."
        }
        $major = [int]$Matches[1]
        $minor = [int]$Matches[2]
        $patch = [int]$Matches[3]
        switch ($Bump) {
            'major' { $major++; $minor = 0; $patch = 0 }
            'minor' { $minor++; $patch = 0 }
            'patch' { $patch++ }
        }
        $newVersion = "$major.$minor.$patch"
    }

    if ($newVersion -eq $currentVersion) {
        throw "Target version ($newVersion) is identical to current version ($currentVersion)."
    }

    $tagName = "v$newVersion"
    $existingTag = git tag --list $tagName
    if (-not [string]::IsNullOrWhiteSpace($existingTag)) {
        throw "Git tag '$tagName' already exists."
    }

    $crateCargoFiles = @(
        Get-ChildItem -Path (Join-Path $repoRoot 'crates') -Filter 'Cargo.toml' -File -Recurse |
            Where-Object { $_.Directory.Name -like 'tableminus-*' } |
            Sort-Object FullName
    )
    if ($crateCargoFiles.Count -eq 0) {
        throw 'No crates/tableminus-*/Cargo.toml files found.'
    }

    Write-Host "Bumping TableMinus version: $currentVersion -> $newVersion"
    foreach ($file in $crateCargoFiles) {
        $rel = $file.FullName.Substring($repoRoot.Length + 1)
        $crateVer = Get-PackageVersion -CargoTomlPath $file.FullName
        if ($DryRun) {
            Write-Host "[DryRun] Would update ${rel}: $crateVer -> $newVersion"
        } else {
            $raw = [System.IO.File]::ReadAllText($file.FullName)
            $updatedToml = Update-PackageVersionInContent -RawText $raw -NewVersion $newVersion
            Write-Utf8NoBom -Path $file.FullName -Content $updatedToml
            Write-Host "Updated ${rel}: $crateVer -> $newVersion"
        }
    }

    if ($DryRun) {
        Write-Host '[DryRun] Would refresh Cargo.lock via: cargo update --workspace --offline'
    } else {
        Write-Host 'Refreshing Cargo.lock...'
        & cargo update --workspace --offline
        if ($LASTEXITCODE -ne 0) {
            Write-Warning 'cargo update --workspace --offline returned non-zero; falling back to cargo metadata --offline.'
            & cargo metadata --offline --format-version 1 > $null
            if ($LASTEXITCODE -ne 0) {
                throw 'Failed to refresh Cargo.lock offline.'
            }
        }
    }

    $latestTag = Get-LatestVersionTag
    $releaseDate = Get-Date -Format 'yyyy-MM-dd'
    $releaseSection = Get-ReleaseSection -NewVersion $newVersion -ReleaseDate $releaseDate -PreviousTag $latestTag

    $changelogPath = Join-Path $repoRoot 'CHANGELOG.md'
    if (Test-Path $changelogPath) {
        $rawChangelog = [System.IO.File]::ReadAllText($changelogPath)
    } else {
        $rawChangelog = "# Changelog`n`n## [Unreleased]`n"
    }
    $updatedChangelog = Update-ChangelogContent -RawChangelog $rawChangelog -NewVersion $newVersion -NewSection $releaseSection

    if ($DryRun) {
        Write-Host "[DryRun] Would prepend section to CHANGELOG.md:"
        Write-Host '---'
        Write-Host $releaseSection
        Write-Host '---'
    } else {
        Write-Utf8NoBom -Path $changelogPath -Content $updatedChangelog
        Write-Host 'Updated CHANGELOG.md'
    }

    $pathsToStage = @($crateCargoFiles | ForEach-Object { $_.FullName.Substring($repoRoot.Length + 1) })
    $pathsToStage += 'Cargo.lock'
    $pathsToStage += 'CHANGELOG.md'
    $commitMessage = "chore(release): prepare $tagName"

    if ($DryRun) {
        Write-Host "[DryRun] Would run: git add $($pathsToStage -join ' ')"
        Write-Host "[DryRun] Would run: git commit -m `"$commitMessage`""
        if ($Tag) {
            Write-Host "[DryRun] Would run: git tag -a $tagName -m `"Release $tagName`""
        }
        if ($Push) {
            $branch = (git rev-parse --abbrev-ref HEAD).Trim()
            Write-Host "[DryRun] Would run: git push origin $branch"
            if ($Tag) {
                Write-Host "[DryRun] Would run: git push origin $tagName"
            }
        }
    } else {
        & git add -- @pathsToStage
        if ($LASTEXITCODE -ne 0) { throw 'git add failed.' }

        & git commit -m $commitMessage
        if ($LASTEXITCODE -ne 0) { throw 'git commit failed.' }

        if ($Tag) {
            & git tag -a $tagName -m "Release $tagName"
            if ($LASTEXITCODE -ne 0) { throw "git tag $tagName failed." }
            Write-Host "Created annotated tag $tagName"
        }

        if ($Push) {
            $branch = (git rev-parse --abbrev-ref HEAD).Trim()
            & git push origin $branch
            if ($LASTEXITCODE -ne 0) { throw "git push origin $branch failed." }
            if ($Tag) {
                & git push origin $tagName
                if ($LASTEXITCODE -ne 0) { throw "git push origin $tagName failed." }
            }
        }
    }

    Write-Host "Done ($tagName)."
} finally {
    Pop-Location
}
