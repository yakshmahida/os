$ErrorActionPreference = 'Stop'

$qemu = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue

if (-not $qemu) {
    $qemuPath = 'C:\Program Files\qemu\qemu-system-x86_64.exe'

    if (-not (Test-Path $qemuPath)) {
        throw 'QEMU was not found. Install QEMU or add it to PATH.'
    }
} else {
    $qemuPath = $qemu.Source
}

# Always rebuild the kernel and boot images.
cargo build

if ($LASTEXITCODE -ne 0) {
    throw 'Build failed. QEMU was not started.'
}

& $qemuPath `
    '-drive' 'format=raw,file=target\micro-os-bios.img' `
    '-serial' 'stdio' `
    '-display' 'gtk' `
    '-no-reboot' `
    '-monitor' 'none'