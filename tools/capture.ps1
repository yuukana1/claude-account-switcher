param(
    [string]$ProcessName = "claude-account-switcher",
    [string]$Out = "shot.png",
    [switch]$NoActivate
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class SwitcherShot {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
# Without this, GetWindowRect returns virtualised 96-DPI coordinates and the crop cuts
# the right/bottom of a scaled window.
[void][SwitcherShot]::SetProcessDPIAware()
$HWND_TOPMOST = [IntPtr](-1)
$HWND_NOTOPMOST = [IntPtr](-2)
$SWP_NOMOVE = 0x0002
$SWP_NOSIZE = 0x0001
$SWP_SHOWWINDOW = 0x0040

$hwnd = [IntPtr]::Zero
$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } |
        Select-Object -First 1
if ($proc) { $hwnd = $proc.MainWindowHandle }
if ($hwnd -eq [IntPtr]::Zero) { Write-Error "window not found for process: $ProcessName"; exit 1 }

if (-not $NoActivate) {
    if ([SwitcherShot]::IsIconic($hwnd)) { [void][SwitcherShot]::ShowWindow($hwnd, 9) }
    [void][SwitcherShot]::SetWindowPos($hwnd, $HWND_TOPMOST, 0, 0, 0, 0, ($SWP_NOMOVE -bor $SWP_NOSIZE -bor $SWP_SHOWWINDOW))
    [void][SwitcherShot]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds 700
}
$rect = New-Object SwitcherShot+RECT
[void][SwitcherShot]::GetWindowRect($hwnd, [ref]$rect)
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) { Write-Error "bad window rect"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
if (-not $NoActivate) {
    [void][SwitcherShot]::SetWindowPos($hwnd, $HWND_NOTOPMOST, 0, 0, 0, 0, ($SWP_NOMOVE -bor $SWP_NOSIZE))
}
Write-Output "$Out ($w x $h)"
