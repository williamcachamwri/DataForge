# Desktop preparation helpers for the screenshot tour on CI runners.
# Dot-source this file, then call the functions (same approach TablePro's CI uses).

Add-Type -AssemblyName System.Windows.Forms
Add-Type -Namespace DfCi -Name Display -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern bool EnumDisplaySettings(string device, int mode, ref DEVMODE dm);
[DllImport("user32.dll", CharSet = CharSet.Unicode)]
public static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct DEVMODE {
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
  public short dmSpecVersion; public short dmDriverVersion; public short dmSize; public short dmDriverExtra;
  public int dmFields;
  public int dmPositionX; public int dmPositionY; public int dmDisplayOrientation; public int dmDisplayFixedOutput;
  public short dmColor; public short dmDuplex; public short dmYResolution; public short dmTTOption; public short dmCollate;
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
  public short dmLogPixels; public int dmBitsPerPel; public int dmPelsWidth; public int dmPelsHeight;
  public int dmDisplayFlags; public int dmDisplayFrequency;
  public int dmICMMethod; public int dmICMIntent; public int dmMediaType; public int dmDitherType;
  public int dmReserved1; public int dmReserved2; public int dmPanningWidth; public int dmPanningHeight;
}
'@

# Switches the primary display to Width x Height so large windows are not clipped.
function Set-Display([int]$Width, [int]$Height) {
    $before = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $dm = New-Object DfCi.Display+DEVMODE
    $dm.dmSize = [Runtime.InteropServices.Marshal]::SizeOf($dm)
    [DfCi.Display]::EnumDisplaySettings($null, -1, [ref]$dm) | Out-Null
    $dm.dmPelsWidth = $Width
    $dm.dmPelsHeight = $Height
    $dm.dmFields = 0x80000 -bor 0x100000
    $code = [DfCi.Display]::ChangeDisplaySettings([ref]$dm, 0)
    Start-Sleep 2
    $after = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    "display before=$($before.Width)x$($before.Height) change=$code after=$($after.Width)x$($after.Height)"
}

# Stops first-logon / privacy dialogs that can cover the app, and keeps them from reopening.
function Clear-FirstLogon {
    try {
        New-Item -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE' -Force | Out-Null
        Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE' -Name 'DisablePrivacyExperience' -Value 1 -Type DWord
    } catch { Write-Warning "OOBE policy not written: $_" }
    foreach ($name in 'WWAHost', 'CloudExperienceHostBroker', 'UserOOBEBroker') {
        Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep 2
    'first-logon dialogs cleared'
}
