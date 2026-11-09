param(
    [int]$X = 0,
    [int]$Y = 0,
    [switch]$Right,
    [switch]$NoActivate
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class SwitcherClick {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[void][SwitcherClick]::SetProcessDPIAware()

$proc = Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.ProcessName -like 'claude*account*' -and $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
if (-not $proc) { Write-Error "app not running"; exit 1 }
$hwnd = $proc.MainWindowHandle
if (-not $NoActivate) {
    [void][SwitcherClick]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0001 -bor 0x0002 -bor 0x0040)
    [void][SwitcherClick]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 250
}
$rect = New-Object SwitcherClick+RECT
[void][SwitcherClick]::GetWindowRect($hwnd, [ref]$rect)
$scale = ($rect.Right - $rect.Left) / 1000.0
$px = [int]($rect.Left + $X * $scale)
$py = [int]($rect.Top + $Y * $scale)
[void][SwitcherClick]::SetCursorPos($px, $py)
Start-Sleep -Milliseconds 120
if ($Right) {
    [SwitcherClick]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
    [SwitcherClick]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
} else {
    [SwitcherClick]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [SwitcherClick]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}
[void][SwitcherClick]::SetWindowPos($hwnd, [IntPtr](-2), 0, 0, 0, 0, 0x0001 -bor 0x0002)
Write-Output "clicked $X,$Y -> $px,$py"
