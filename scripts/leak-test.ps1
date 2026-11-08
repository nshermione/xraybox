# Runtime leak checks for the Windows build.
#
# Unit tests prove the config and the firewall arguments. They cannot prove what
# Windows actually does with them, and every leak this script looks for is a
# routing-table or resolver fact that only exists on a live machine.
#
# Run it twice: once disconnected to capture a baseline, once connected.
#
#   .\scripts\leak-test.ps1                          # auto-detects the state
#   .\scripts\leak-test.ps1 -BaselineIp 1.2.3.4      # fails if the IP is unchanged
#   .\scripts\leak-test.ps1 -IncludeCrashTest        # kills xray.exe, needs admin
#
# Exit code 0 means every applicable check passed. Read-only unless
# -IncludeCrashTest is given.

[CmdletBinding()]
param(
    [switch]$IncludeCrashTest,
    [string]$BaselineIp,
    [int]$TimeoutSec = 6
)

$ErrorActionPreference = "Stop"

$TunAdapterName = "xraybox-tun"
$TunPrefix = "172.19.0.0/30"
$GuardRuleNames = @(
    "Xraybox Leak Guard IPv6",
    "Xraybox Leak Guard Off-Tunnel DNS UDP",
    "Xraybox Leak Guard Off-Tunnel DNS TCP"
)
$OffTunnelRanges = @(
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "100.64.0.0/10",
    "169.254.0.0/16"
)
$TraceUrl = "http://cp.cloudflare.com/cdn-cgi/trace"
$Ipv6Probe = "2606:4700:4700::1111"

$results = [System.Collections.Generic.List[object]]::new()

function Add-Result {
    param(
        [Parameter(Mandatory)][string]$Check,
        [Parameter(Mandatory)][ValidateSet("PASS", "FAIL", "WARN", "SKIP", "INFO")][string]$Status,
        [string]$Detail = ""
    )
    $results.Add([pscustomobject]@{ Check = $Check; Status = $Status; Detail = $Detail })
    $color = switch ($Status) {
        "PASS" { "Green" }
        "FAIL" { "Red" }
        "WARN" { "Yellow" }
        "SKIP" { "DarkGray" }
        default { "Gray" }
    }
    Write-Host ("{0,-5} {1}" -f $Status, $Check) -ForegroundColor $color
    if ($Detail) {
        Write-Host ("      {0}" -f $Detail) -ForegroundColor DarkGray
    }
}

function Convert-IPv4ToInt64 {
    param([string]$Address)
    $bytes = [System.Net.IPAddress]::Parse($Address).GetAddressBytes()
    return ([long]$bytes[0] * 16777216) + ([long]$bytes[1] * 65536) + ([long]$bytes[2] * 256) + [long]$bytes[3]
}

function Test-IPv4InCidr {
    param([string]$Address, [string]$Cidr)
    $parts = $Cidr.Split("/")
    $bits = [int]$parts[1]
    if ($bits -le 0) { return $true }
    $mask = [long]([math]::Pow(2, 32) - [math]::Pow(2, 32 - $bits))
    return ((Convert-IPv4ToInt64 $Address) -band $mask) -eq ((Convert-IPv4ToInt64 $parts[0]) -band $mask)
}

function Test-OffTunnelResolver {
    param([string]$Address)
    if ($Address -notmatch '^\d+\.\d+\.\d+\.\d+$') { return $false }
    foreach ($range in $OffTunnelRanges) {
        if (Test-IPv4InCidr -Address $Address -Cidr $range) { return $true }
    }
    return $false
}

function Get-TraceInfo {
    try {
        $body = (Invoke-WebRequest -Uri $TraceUrl -UseBasicParsing -TimeoutSec $TimeoutSec).Content
    } catch {
        return $null
    }
    $info = @{}
    foreach ($line in $body -split "`n") {
        $pair = $line -split "=", 2
        if ($pair.Count -eq 2) { $info[$pair[0].Trim()] = $pair[1].Trim() }
    }
    return $info
}

function Get-XrayProcess {
    # Release bundles ship xray.exe; a dev build runs the target-triple name.
    return @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like "xray*" })
}

function Get-TunnelState {
    $adapter = @(Get-NetAdapter -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like "*$TunAdapterName*" -or $_.InterfaceDescription -like "*$TunAdapterName*" })
    $process = Get-XrayProcess
    return [pscustomobject]@{
        Adapter   = $adapter | Select-Object -First 1
        Process   = $process
        Connected = ($adapter.Count -gt 0) -and ($process.Count -gt 0)
    }
}

Write-Host "=== xraybox leak checks ===" -ForegroundColor Cyan

$state = Get-TunnelState
if ($state.Connected) {
    Add-Result -Check "Tunnel state" -Status "INFO" -Detail "connected: adapter '$($state.Adapter.Name)', xray pid $($state.Process.Id -join ',')"
} else {
    $why = if ($state.Process.Count -eq 0) { "xray.exe not running" } else { "adapter '$TunAdapterName' not found" }
    Add-Result -Check "Tunnel state" -Status "INFO" -Detail "disconnected ($why) - connected-only checks will be skipped"
}

# --- 1. TUN adapter has resolvers of its own -----------------------------------
# Without settings.dns on the TUN inbound the adapter has no resolver and Windows
# falls back to the physical NIC's servers, which for a LAN address never crosses
# the tunnel.
if ($state.Connected) {
    $tunDns = (Get-DnsClientServerAddress -InterfaceIndex $state.Adapter.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue).ServerAddresses
    if (-not $tunDns -or $tunDns.Count -eq 0) {
        Add-Result -Check "TUN adapter has DNS servers" -Status "FAIL" -Detail "none assigned; the OS will resolve through the physical NIC"
    } else {
        $offTunnel = @($tunDns | Where-Object { Test-OffTunnelResolver $_ })
        if ($offTunnel.Count -gt 0) {
            Add-Result -Check "TUN adapter DNS servers are reachable only through the tunnel" -Status "FAIL" -Detail "off-tunnel resolver assigned: $($offTunnel -join ', ')"
        } else {
            Add-Result -Check "TUN adapter has DNS servers" -Status "PASS" -Detail ($tunDns -join ", ")
        }
    }
} else {
    Add-Result -Check "TUN adapter has DNS servers" -Status "SKIP"
}

# --- 2. Off-tunnel resolvers are unreachable ----------------------------------
# The real test of the DNS guard: ask a LAN resolver directly. While connected it
# must not answer.
$lanResolvers = @(
    Get-DnsClientServerAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.InterfaceAlias -notlike "*$TunAdapterName*" -and $_.InterfaceAlias -notlike "Loopback*" } |
        Select-Object -ExpandProperty ServerAddresses |
        Where-Object { Test-OffTunnelResolver $_ } |
        Select-Object -Unique
)
if (-not $state.Connected) {
    Add-Result -Check "Off-tunnel resolvers are blocked" -Status "SKIP" -Detail "found while disconnected: $($lanResolvers -join ', ')"
} elseif ($lanResolvers.Count -eq 0) {
    Add-Result -Check "Off-tunnel resolvers are blocked" -Status "PASS" -Detail "no LAN resolver configured on any physical adapter"
} else {
    $answered = @()
    foreach ($resolver in $lanResolvers) {
        try {
            $null = Resolve-DnsName -Name "example.com" -Server $resolver -Type A -DnsOnly -QuickTimeout -ErrorAction Stop
            $answered += $resolver
        } catch {
            # Expected: the guard rule drops the query.
        }
    }
    if ($answered.Count -gt 0) {
        Add-Result -Check "Off-tunnel resolvers are blocked" -Status "FAIL" -Detail "answered outside the tunnel: $($answered -join ', ')"
    } else {
        Add-Result -Check "Off-tunnel resolvers are blocked" -Status "PASS" -Detail "no answer from $($lanResolvers -join ', ')"
    }
}

# --- 3. Global IPv6 cannot leave --------------------------------------------
# The TUN carries 0.0.0.0/0 only, so any working IPv6 path is a path around it.
$globalIpv6 = @(
    Get-NetIPAddress -AddressFamily IPv6 -ErrorAction SilentlyContinue |
        Where-Object { $_.IPAddress -notlike "fe80*" -and $_.IPAddress -ne "::1" -and $_.PrefixOrigin -ne "WellKnown" }
)
$ipv6DefaultRoute = @(Get-NetRoute -DestinationPrefix "::/0" -ErrorAction SilentlyContinue)

if ($globalIpv6.Count -eq 0 -and $ipv6DefaultRoute.Count -eq 0) {
    Add-Result -Check "Global IPv6 cannot leave" -Status "PASS" -Detail "no global IPv6 address and no ::/0 route on this network"
} elseif (-not $state.Connected) {
    Add-Result -Check "Global IPv6 cannot leave" -Status "SKIP" -Detail "IPv6 present ($($globalIpv6.IPAddress -join ', ')); rerun while connected"
} else {
    $reached = $false
    try {
        $reached = (Test-NetConnection -ComputerName $Ipv6Probe -Port 443 -InformationLevel Quiet -WarningAction SilentlyContinue)
    } catch {
        $reached = $false
    }
    if ($reached) {
        Add-Result -Check "Global IPv6 cannot leave" -Status "FAIL" -Detail "reached $Ipv6Probe over native IPv6 - the remote host sees the real address"
    } else {
        Add-Result -Check "Global IPv6 cannot leave" -Status "PASS" -Detail "$Ipv6Probe unreachable while connected"
    }
}

# --- 4. Guard rules are installed -------------------------------------------
$installed = @()
foreach ($name in $GuardRuleNames) {
    if (Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue) { $installed += $name }
}
if ($state.Connected) {
    if ($installed.Count -eq $GuardRuleNames.Count) {
        Add-Result -Check "Leak guard firewall rules installed" -Status "PASS" -Detail "$($installed.Count)/$($GuardRuleNames.Count)"
    } else {
        $missing = $GuardRuleNames | Where-Object { $installed -notcontains $_ }
        Add-Result -Check "Leak guard firewall rules installed" -Status "FAIL" -Detail "missing: $($missing -join '; ')"
    }
} else {
    if ($installed.Count -eq 0) {
        Add-Result -Check "Leak guard firewall rules released when disconnected" -Status "PASS"
    } else {
        Add-Result -Check "Leak guard firewall rules released when disconnected" -Status "FAIL" -Detail "left behind: $($installed -join '; ') - IPv6 and LAN DNS stay blocked, and firewall rules survive a reboot"
    }
}

# --- 5. Egress address ------------------------------------------------------
$trace = Get-TraceInfo
if ($null -eq $trace) {
    $status = if ($state.Connected) { "FAIL" } else { "WARN" }
    Add-Result -Check "Egress address reachable" -Status $status -Detail "no answer from $TraceUrl"
} else {
    Add-Result -Check "Egress address" -Status "INFO" -Detail "ip=$($trace['ip']) loc=$($trace['loc'])"
    if ($BaselineIp) {
        if ($trace["ip"] -eq $BaselineIp) {
            $status = if ($state.Connected) { "FAIL" } else { "PASS" }
            $detail = if ($state.Connected) { "egress still matches the disconnected baseline $BaselineIp" } else { "matches baseline, as expected while disconnected" }
            Add-Result -Check "Egress differs from the disconnected baseline" -Status $status -Detail $detail
        } else {
            Add-Result -Check "Egress differs from the disconnected baseline" -Status "PASS" -Detail "$BaselineIp -> $($trace['ip'])"
        }
    } elseif (-not $state.Connected) {
        Add-Result -Check "Baseline captured" -Status "INFO" -Detail "rerun connected with -BaselineIp $($trace['ip'])"
    }
}

# --- 6. Route sanity --------------------------------------------------------
if ($state.Connected) {
    $tunDefault = @(Get-NetRoute -DestinationPrefix "0.0.0.0/0" -ErrorAction SilentlyContinue |
        Where-Object { $_.InterfaceIndex -eq $state.Adapter.ifIndex })
    if ($tunDefault.Count -gt 0) {
        Add-Result -Check "TUN owns an IPv4 default route" -Status "PASS" -Detail "metric $($tunDefault[0].RouteMetric), ifMetric $($tunDefault[0].InterfaceMetric)"
    } else {
        Add-Result -Check "TUN owns an IPv4 default route" -Status "FAIL" -Detail "no 0.0.0.0/0 route on '$($state.Adapter.Name)'; traffic is not entering the tunnel"
    }
} else {
    Add-Result -Check "TUN owns an IPv4 default route" -Status "SKIP"
}

# --- 7. Fail-closed on crash ------------------------------------------------
if ($IncludeCrashTest) {
    if (-not $state.Connected) {
        Add-Result -Check "Fail closed when the sidecar dies" -Status "SKIP" -Detail "not connected"
    } else {
        Write-Host "Killing xray.exe to test the fail-closed path..." -ForegroundColor Yellow
        Get-XrayProcess | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 3
        $afterRules = @()
        foreach ($name in $GuardRuleNames) {
            if (Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue) { $afterRules += $name }
        }
        if ($afterRules.Count -eq $GuardRuleNames.Count) {
            Add-Result -Check "Leak guard survives a sidecar crash" -Status "PASS"
        } else {
            Add-Result -Check "Leak guard survives a sidecar crash" -Status "FAIL" -Detail "released on crash; IPv6 and LAN DNS are open again"
        }
        $afterTrace = Get-TraceInfo
        if ($null -eq $afterTrace) {
            Add-Result -Check "Egress after crash" -Status "PASS" -Detail "no egress at all - kill switch held"
        } else {
            $detail = "ip=$($afterTrace['ip']) loc=$($afterTrace['loc'])"
            if ($trace -and $afterTrace["ip"] -eq $trace["ip"]) {
                Add-Result -Check "Egress after crash" -Status "WARN" -Detail "$detail - unchanged, verify against the disconnected baseline"
            } else {
                Add-Result -Check "Egress after crash" -Status "FAIL" -Detail "$detail - traffic fell back to the physical NIC; enable the kill switch"
            }
        }
        Write-Host "Reconnect from the app to restore the tunnel." -ForegroundColor Yellow
    }
}

Write-Host ""
Write-Host "=== Summary ===" -ForegroundColor Cyan
$results | Format-Table -Property Status, Check, Detail -AutoSize

$failed = @($results | Where-Object { $_.Status -eq "FAIL" })
if ($failed.Count -gt 0) {
    Write-Host "$($failed.Count) check(s) FAILED" -ForegroundColor Red
    exit 1
}
Write-Host "No failures" -ForegroundColor Green
exit 0
