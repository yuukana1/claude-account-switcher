param(
    [string]$Keys = "~",
    [int]$Delay = 400
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class SwitcherKeys {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
}
"@
$KEYEVENTF_KEYUP = 0x0002
$KEYEVENTF_UNICODE = 0x0004

$proc = Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ProcessName -like 'claude*account*' -and $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
if (-not $proc) { Write-Error "app not running"; exit 1 }
$hwnd = $proc.MainWindowHandle
[void][SwitcherKeys]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0001 -bor 0x0002 -bor 0x0040)
[void][SwitcherKeys]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds $Delay

$map = @{
    'RETURN' = 0x0D; 'ENTER' = 0x0D; 'ESC' = 0x1B; 'TAB' = 0x09;
    'BACK' = 0x08; 'DELETE' = 0x2E; 'UP' = 0x26; 'DOWN' = 0x28;
}
$code = $Keys.ToUpper()
if ($map.ContainsKey($code)) {
    $vk = [byte]$map[$code]
    [SwitcherKeys]::keybd_event($vk, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [SwitcherKeys]::keybd_event($vk, 0, $KEYEVENTF_KEYUP, [UIntPtr]::Zero)
} else {
    foreach ($ch in $Keys.ToCharArray()) {
        $scan = [byte][char]$ch
        [SwitcherKeys]::keybd_event(0, $scan, $KEYEVENTF_UNICODE, [UIntPtr]::Zero)
        [SwitcherKeys]::keybd_event(0, $scan, ($KEYEVENTF_UNICODE -bor $KEYEVENTF_KEYUP), [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
    }
}
Start-Sleep -Milliseconds 400
[void][SwitcherKeys]::SetWindowPos($hwnd, [IntPtr](-2), 0, 0, 0, 0, 0x0001 -bor 0x0002)
Write-Output "sent $Keys"
