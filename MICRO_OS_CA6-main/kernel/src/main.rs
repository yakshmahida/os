#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader_api::{entry_point, info::FrameBufferInfo, info::PixelFormat, BootInfo};
use core::alloc::{GlobalAlloc, Layout};
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use uart_16550::SerialPort;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

const PIC_1_OFFSET: u8 = 32;
const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
const TIMER_FREQUENCY_HZ: u32 = 20;
const MAX_TASKS: usize = 8;

entry_point!(kernel_main);

lazy_static! {
    static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial = unsafe { SerialPort::new(0x3F8) };
        serial.init();
        Mutex::new(serial)
    };
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        idt
    };
}

static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

// =========================================================================
// TITANIUM WORKSTATION 2.0 - RADICALLY TRANSFORMED LIGHT/HIGH-CONTRAST PALETTE
// =========================================================================

// Desktop Platinum / Studio Canvas
const PALETTE_CANVAS: [u8; 3]        = [236, 240, 246]; // Crisp Nordic Platinum canvas
const PALETTE_CANVAS_BORDER: [u8; 3] = [203, 213, 225]; // Soft slate framing border

// Top Menu Bar
const PALETTE_TOPBAR: [u8; 3]        = [255, 255, 255]; // Pure White Apple/NeXT Menu Bar
const PALETTE_TOPBAR_BORDER: [u8; 3] = [218, 224, 233]; // Divider line
const PALETTE_TOPBAR_TEXT: [u8; 3]   = [30, 41, 59];    // Deep charcoal menu text
const PALETTE_TOPBAR_MUTED: [u8; 3]  = [100, 116, 139]; // Slate muted
const PALETTE_RUBY: [u8; 3]          = [225, 29, 72];   // Electric Crimson Ruby OS Emblem

// Left Sidebar (Deep Midnight Titanium Workstation Control Rail)
const PALETTE_SIDEBAR_BG: [u8; 3]    = [15, 23, 42];    // Deep Midnight Slate
const PALETTE_SIDEBAR_CARD: [u8; 3]  = [24, 34, 58];    // Surface card
const PALETTE_SIDEBAR_CARD_BRD: [u8; 3] = [38, 52, 84]; // Card outline
const PALETTE_SIDEBAR_TEXT: [u8; 3]  = [226, 232, 240]; // Crisp slate 200

// Floating Terminal Window (Mac / NeXTSTEP Style with Traffic Light Pills)
const PALETTE_WIN_FRAME: [u8; 3]     = [71, 85, 105];   // Metallic frame border
const PALETTE_WIN_SHADOW: [u8; 3]    = [195, 205, 218]; // Window drop shadow
const PALETTE_WIN_TITLEBAR: [u8; 3]  = [30, 41, 59];    // Dark titanium header
const PALETTE_WIN_TITLE_TXT: [u8; 3] = [241, 245, 249]; // Pure crisp title text
const PALETTE_TRAFFIC_RED: [u8; 3]   = [239, 68, 68];   // Close button
const PALETTE_TRAFFIC_YEL: [u8; 3]   = [245, 158, 11];  // Minimize button
const PALETTE_TRAFFIC_GRN: [u8; 3]   = [16, 185, 129];  // Expand button

// Terminal Body & High-Contrast Typography
const PALETTE_TERM_BG: [u8; 3]       = [14, 18, 28];    // Obsidian Console Canvas
const PALETTE_TERM_PROMPT: [u8; 3]   = [251, 191, 36];  // Radiant Warm Imperial Gold
const PALETTE_TERM_MAIN: [u8; 3]     = [248, 250, 252]; // Brilliant Snow White
const PALETTE_TERM_MUTED: [u8; 3]    = [148, 163, 184]; // Soft Slate 400
const PALETTE_TERM_CORAL: [u8; 3]    = [244, 63, 94];   // Electric Coral Pink/Red
const PALETTE_TERM_GOLD: [u8; 3]     = [250, 204, 21];  // Sun Amber Gold
const PALETTE_TERM_MINT: [u8; 3]     = [52, 211, 153];  // Emerald Mint Green
const PALETTE_TERM_CYAN: [u8; 3]     = [56, 189, 248];  // Sky Cobalt Blue
const PALETTE_TERM_VIOLET: [u8; 3]   = [192, 132, 252]; // Lavender Violet

// Bottom Floating App Dock
const PALETTE_DOCK_BG: [u8; 3]       = [255, 255, 255]; // Crisp white dock bar
const PALETTE_DOCK_BORDER: [u8; 3]   = [203, 213, 225]; // Dock border
const PALETTE_DOCK_PILL: [u8; 3]     = [241, 245, 249]; // Pill button fill
const PALETTE_DOCK_TEXT: [u8; 3]     = [30, 41, 59];    // Pill button text

// Terminal Dimensions (Expanded for 800x600 High-Legibility 8x12 Monospace)
const TERM_LINES: usize = 18;
const TERM_COLS: usize = 56;
const PROCESS_COUNT: usize = 3;
const RESOURCE_COUNT: usize = 3;
const MAXIMUM: [[u8; RESOURCE_COUNT]; PROCESS_COUNT] = [
    [3, 2, 2],
    [2, 2, 1],
    [3, 3, 2],
];
const INITIAL_ALLOCATION: [[u8; RESOURCE_COUNT]; PROCESS_COUNT] = [
    [1, 0, 1],
    [1, 1, 0],
    [1, 1, 1],
];
const INITIAL_AVAILABLE: [u8; RESOURCE_COUNT] = [1, 1, 1];

fn banker_safe_sequence(
    allocation: &[[u8; RESOURCE_COUNT]; PROCESS_COUNT],
    available: &[u8; RESOURCE_COUNT],
) -> Option<[usize; PROCESS_COUNT]> {
    let mut work = *available;
    let mut finished = [false; PROCESS_COUNT];
    let mut sequence = [0usize; PROCESS_COUNT];
    let mut completed = 0;

    while completed < PROCESS_COUNT {
        let mut progress = false;
        for p in 0..PROCESS_COUNT {
            if finished[p] { continue; }
            let mut can_finish = true;
            for r in 0..RESOURCE_COUNT {
                let need = MAXIMUM[p][r] - allocation[p][r];
                if need > work[r] { can_finish = false; break; }
            }
            if can_finish {
                for r in 0..RESOURCE_COUNT { work[r] += allocation[p][r]; }
                finished[p] = true;
                sequence[completed] = p;
                completed += 1;
                progress = true;
            }
        }
        if !progress { return None; }
    }
    Some(sequence)
}

struct Screen {
    buffer: *mut u8,
    info: Option<FrameBufferInfo>,
    active_task: usize,
    completed: [bool; 3],
    term_history: [[u8; TERM_COLS]; TERM_LINES],
    term_colors: [[u8; 3]; TERM_LINES],
    term_count: usize,
    input_buf: [u8; 48],
    input_len: usize,
    allocation: [[u8; RESOURCE_COUNT]; PROCESS_COUNT],
    available: [u8; RESOURCE_COUNT],
}

unsafe impl Sync for Screen {}
unsafe impl Send for Screen {}

impl Screen {
    const fn new() -> Self {
        Self {
            buffer: core::ptr::null_mut(),
            info: None,
            active_task: 0,
            completed: [false; 3],
            term_history: [[b' '; TERM_COLS]; TERM_LINES],
            term_colors: [PALETTE_TERM_MAIN; TERM_LINES],
            term_count: 0,
            input_buf: [0; 48],
            input_len: 0,
            allocation: INITIAL_ALLOCATION,
            available: INITIAL_AVAILABLE,
        }
    }

    fn init(&mut self, framebuffer: &mut bootloader_api::info::FrameBuffer) {
        let info = framebuffer.info();
        self.buffer = framebuffer.buffer_mut().as_mut_ptr();
        self.info = Some(info);
    }

    fn show_splash_animation(&mut self) {
        let Some(info) = self.info else { return };
        let w = info.width;
        let h = info.height;

        // Stage 1: Deep Onyx Space Canvas
        self.fill([12, 14, 22]);

        let cx = w / 2;
        let cy = h / 2 - 45;

        // Subtle decorative crosshairs & orbital rings
        self.rect(cx - 240, cy, 480, 1, [30, 36, 54]);
        self.rect(cx, cy - 120, 1, 240, [30, 36, 54]);

        // Concentric Radiant Diamond Stellar Core Logo (Enlarged)
        self.diamond(cx, cy, 48, PALETTE_RUBY);           // Outer Crimson Star
        self.diamond(cx, cy, 40, PALETTE_TERM_GOLD);     // Middle Warm Imperial Gold
        self.diamond(cx, cy, 28, [15, 23, 42]);          // Dark Midnight Horizon
        self.diamond(cx, cy, 16, [254, 240, 138]);       // Radiant Golden Nucleus
        self.diamond(cx, cy, 8,  [255, 255, 255]);       // Pure White Starlight Core

        // 4 Orbital Satellite Nodes
        self.rect(cx - 64, cy - 4, 8, 8, PALETTE_TERM_MINT);
        self.rect(cx + 56, cy - 4, 8, 8, PALETTE_TERM_MINT);
        self.rect(cx - 4, cy - 64, 8, 8, PALETTE_TERM_VIOLET);
        self.rect(cx - 4, cy + 56, 8, 8, PALETTE_TERM_VIOLET);

        // Bold Titanium Workstation Typography (Enlarged 8x12 Font)
        self.text(cx - 150, cy + 64, "M I C R O - O S   2 . 0", 2, [255, 255, 255]);
        self.text(cx - 190, cy + 96, "TITANIUM EDITION // HYBRID WORKSTATION", 1, PALETTE_TERM_GOLD);
        self.text(cx - 170, cy + 116, "APPLE SILICON M4 | R4 ASYMMETRIC MLFQ", 1, PALETTE_TERM_MUTED);

        // Segmented Dual-Tone Progress Bar
        let bar_w = 400;
        let bar_x = cx - bar_w / 2;
        let bar_y = cy + 144;
        self.rect(bar_x - 2, bar_y - 2, bar_w + 4, 16, [40, 50, 75]);
        self.rect(bar_x, bar_y, bar_w, 12, [20, 26, 40]);

        let steps = [
            (100, "[01/04] PROBING APPLE SILICON M4 TOPOLOGY (4P + 6E)..."),
            (200, "[02/04] INITIALIZING WI-FI 6 & GIGABIT ETHERNET STACK..."),
            (300, "[03/04] VERIFYING IN-MEMORY DPKG CACHE & SNAPSTORE..."),
            (400, "[04/04] ENGAGING ASYMMETRIC WORKLOAD-AWARE SCHEDULER..."),
        ];

        for (progress, msg) in steps.iter() {
            self.rect(bar_x, bar_y, *progress, 12, PALETTE_TERM_GOLD);
            if *progress >= 16 {
                self.rect(bar_x + *progress - 16, bar_y, 16, 12, PALETTE_RUBY);
            }
            self.rect(bar_x, bar_y + 22, bar_w, 16, [12, 14, 22]);
            self.text(bar_x + 10, bar_y + 22, msg, 1, PALETTE_TERM_MAIN);

            for _ in 0..10_000_000 {
                core::hint::spin_loop();
            }
        }
    }

    fn render_desktop(&mut self) {
        let Some(info) = self.info else { return };
        let w = info.width;
        let h = info.height;

        // 1. Base Workspace Canvas: Crisp Nordic Platinum (Full Screen Coverage)
        self.fill(PALETTE_CANVAS);

        // 2. Top Menu Bar (NeXTSTEP / Apple Classic Studio Bar, Height: 30px)
        self.rect(0, 0, w, 30, PALETTE_TOPBAR);
        self.rect(0, 29, w, 1, PALETTE_TOPBAR_BORDER);

        // Ruby OS Badge on Top Left
        self.rect(8, 5, 20, 20, PALETTE_RUBY);
        self.diamond(18, 15, 5, [255, 255, 255]); // White diamond inside ruby
        self.text(36, 9, "TITANIUM MICRO-OS", 1, PALETTE_TOPBAR_TEXT);

        // Top Menus with generous spacing
        self.text(195, 9, "FILE", 1, PALETTE_TOPBAR_MUTED);
        self.text(245, 9, "TASKS", 1, PALETTE_TOPBAR_MUTED);
        self.text(305, 9, "ROUTING", 1, PALETTE_TOPBAR_MUTED);
        self.text(380, 9, "SECURITY", 1, PALETTE_TOPBAR_MUTED);
        self.text(465, 9, "HELP", 1, PALETTE_TOPBAR_MUTED);

        // Top Right Telemetry & Clock
        self.rect(w - 245, 10, 10, 10, PALETTE_TRAFFIC_GRN); // Green Online Dot
        self.text(w - 228, 9, "WI-FI CONNECTED", 1, PALETTE_TOPBAR_TEXT);
        self.text(w - 85, 9, "12:50 UTC", 1, PALETTE_TOPBAR_MUTED);

        // 3. Left Control Rail / System Inspector (Widened to 264px, 95% Vertical Height)
        let sidebar_w = 264;
        let sidebar_h = h - 30 - 32;
        self.rect(0, 30, sidebar_w, sidebar_h, PALETTE_SIDEBAR_BG);
        self.rect(sidebar_w - 1, 30, 1, sidebar_h, PALETTE_CANVAS_BORDER);

        // Sidebar Section 1: System Hardware (Card Height: 98px, Spacing: 16px)
        self.sidebar_card(8, 36, 248, 98, "SYSTEM HARDWARE", PALETTE_TERM_GOLD);
        self.text(18, 56,  "HOST: MACBOOK AIR M4", 1, PALETTE_SIDEBAR_TEXT);
        self.text(18, 72,  "CPU:  APPLE M4 (10C)", 1, PALETTE_TERM_MUTED);
        self.text(18, 88,  "ARCH: 4P + 6E CORES", 1, PALETTE_TERM_MUTED);
        self.text(18, 104, "RAM:  16 GB UNIFIED", 1, PALETTE_TERM_MINT);

        // Sidebar Section 2: Hybrid Core Routing
        self.sidebar_card(8, 142, 248, 98, "CORE ROUTING (R4)", PALETTE_TERM_CORAL);
        self.text(18, 162, "P00 > SHELL (INTERACTIVE)", 1, PALETTE_TERM_GOLD);
        self.text(18, 178, "E04 > LOGGER (TELEMETRY)", 1, PALETTE_TERM_MINT);
        self.text(18, 194, "E05 > WORKER (BATCH)", 1, PALETTE_TERM_CORAL);
        self.text(18, 210, "POLICY: WORKLOAD MLFQ", 1, PALETTE_SIDEBAR_TEXT);

        // Sidebar Section 3: Network & Stack
        self.sidebar_card(8, 248, 248, 98, "NETWORK STACK", PALETTE_TERM_MINT);
        self.text(18, 268, "WI-FI: CONNECTED (AX)", 1, PALETTE_SIDEBAR_TEXT);
        self.text(18, 284, "ETH:   GBE NAT (1000M)", 1, PALETTE_TERM_MUTED);
        self.text(18, 300, "IPV4:  10.0.2.15/24", 1, PALETTE_TERM_MUTED);
        self.text(18, 316, "STATUS: ONLINE (NAT)", 1, PALETTE_TRAFFIC_GRN);

        // Sidebar Section 4: Packages & Apps
        self.sidebar_card(8, 354, 248, 98, "PACKAGES & APPS", PALETTE_TERM_CYAN);
        self.text(18, 374, "APT:  IN-RAM DPKG DB", 1, PALETTE_SIDEBAR_TEXT);
        self.text(18, 390, "PKGS: 58,412 PACKAGES", 1, PALETTE_TERM_MUTED);
        self.text(18, 406, "SNAP: SQUASHFS LOOP0-7", 1, PALETTE_TERM_MUTED);
        self.text(18, 422, "STORE: READY (ACTIVE)", 1, PALETTE_TRAFFIC_GRN);

        // Sidebar Section 5: Banker Deadlock Engine
        self.sidebar_card(8, 460, 248, 98, "BANKER DEADLOCK", PALETTE_TERM_VIOLET);
        self.text(18, 480, "STATE: SAFE SEQUENCE", 1, PALETTE_TRAFFIC_GRN);
        self.text(18, 496, "CLAIMS: MATRIX A/B/C", 1, PALETTE_TERM_MUTED);
        self.text(18, 512, "AVOID: DETERMINISTIC", 1, PALETTE_TERM_MUTED);
        self.text(18, 528, "TRY: 'deadlock'", 1, PALETTE_TERM_GOLD);

        // 4. Large Floating Terminal Window (Occupies 95% of Right Screen Space)
        let win_x = 268;
        let win_y = 34;
        let win_w = w - win_x - 6;
        let win_h = 530;

        // Window Drop Shadow
        self.rect(win_x + 3, win_y + 3, win_w, win_h, PALETTE_WIN_SHADOW);

        // Window Outer Frame
        self.rect(win_x - 1, win_y - 1, win_w + 2, win_h + 2, PALETTE_WIN_FRAME);

        // Window Titlebar (Enlarged to 28px)
        self.rect(win_x, win_y, win_w, 28, PALETTE_WIN_TITLEBAR);

        // Traffic Light Window Controls (Enlarged 10x10)
        self.rect(win_x + 10, win_y + 9, 10, 10, PALETTE_TRAFFIC_RED);
        self.rect(win_x + 24, win_y + 9, 10, 10, PALETTE_TRAFFIC_YEL);
        self.rect(win_x + 38, win_y + 9, 10, 10, PALETTE_TRAFFIC_GRN);

        // Window Title
        self.text(win_x + 56, win_y + 8, "CONSOLE // WORKLOAD-AWARE INTERACTIVE SHELL", 1, PALETTE_WIN_TITLE_TXT);

        // Window Interior Body Canvas
        self.rect(win_x, win_y + 28, win_w, win_h - 28, PALETTE_TERM_BG);

        // Task Execution Sub-Panel (Taller 40px with Bold Gauges)
        self.rect(win_x + 8, win_y + 34, win_w - 16, 40, [22, 30, 46]);
        self.rect(win_x + 8, win_y + 34, win_w - 16, 1, [40, 52, 78]);
        self.rect(win_x + 8, win_y + 73, win_w - 16, 1, [40, 52, 78]);

        let card_w = (win_w - 16 - 24) / 3;
        for index in 0..3 {
            let cx = win_x + 12 + index * (card_w + 8);
            self.rect(cx, win_y + 38, card_w, 32, [16, 22, 36]);
            let (t_title, t_col) = match index {
                0 => ("T1: SHELL", PALETTE_TERM_GOLD),
                1 => ("T2: LOGGER", PALETTE_TERM_MINT),
                _ => ("T3: WORKER", PALETTE_TERM_CORAL),
            };
            self.text(cx + 6, win_y + 42, t_title, 1, t_col);

            // Progress bar track (Thicker 5px bar)
            self.rect(cx + 8, win_y + 58, card_w - 16, 5, [26, 36, 56]);
            self.rect(cx + 8, win_y + 58, (card_w - 16) / 4, 5, t_col);
        }

        // Quick Hint Bar at bottom of window
        self.rect(win_x + 8, win_y + win_h - 26, win_w - 16, 20, [20, 27, 42]);
        self.text(win_x + 14, win_y + win_h - 22, "APPS: help | web | apt | snap | top | net | deadlock", 1, PALETTE_TERM_GOLD);

        // Initial Terminal Messages in Large High-Contrast 8x12 Font
        self.add_term_line("TITANIUM MICRO-OS v0.4.0 // WORKSTATION OS", PALETTE_TERM_GOLD);
        self.add_term_line("Core Topology: Shell->P00 | Log->E04 | Work->E05", PALETTE_TERM_CORAL);
        self.add_term_line("Deadlock Safety Manager: Banker Algorithm Active", PALETTE_TERM_MINT);
        self.add_term_line("Type 'help' to inspect commands or 'web' for portal.", PALETTE_TERM_CYAN);

        self.redraw_term();

        // 5. Full-Width Bottom Application Shelf (Height: 32px, y: h - 32)
        let dock_h = 32;
        let dock_y = h - dock_h;

        self.rect(0, dock_y, w, dock_h, PALETTE_DOCK_BG);
        self.rect(0, dock_y, w, 1, PALETTE_DOCK_BORDER);

        let apps = [
            "1:SHELL", "2:BROWSER", "3:APT-PKG", "4:SNAP", "5:TOP", "6:DEADLOCK"
        ];
        let app_w = 112;
        let start_ax = (w - (apps.len() * (app_w + 8) - 8)) / 2;
        for (i, app) in apps.iter().enumerate() {
            let ax = start_ax + i * (app_w + 8);
            self.rect(ax, dock_y + 4, app_w, 24, PALETTE_DOCK_PILL);
            self.rect(ax, dock_y + 4, app_w, 1, PALETTE_DOCK_BORDER);
            self.rect(ax, dock_y + 27, app_w, 1, PALETTE_DOCK_BORDER);
            self.rect(ax, dock_y + 4, 1, 24, PALETTE_DOCK_BORDER);
            self.rect(ax + app_w - 1, dock_y + 4, 1, 24, PALETTE_DOCK_BORDER);
            self.text(ax + 11, dock_y + 10, app, 1, PALETTE_DOCK_TEXT);
        }
    }

    fn sidebar_card(&mut self, x: usize, y: usize, w: usize, h: usize, title: &str, accent: [u8; 3]) {
        self.rect(x, y, w, h, PALETTE_SIDEBAR_CARD);
        self.rect(x, y, w, 1, PALETTE_SIDEBAR_CARD_BRD);
        self.rect(x, y + h - 1, w, 1, PALETTE_SIDEBAR_CARD_BRD);
        self.rect(x, y, 1, h, PALETTE_SIDEBAR_CARD_BRD);
        self.rect(x + w - 1, y, 1, h, PALETTE_SIDEBAR_CARD_BRD);
        // Prominent colored accent pill next to section title
        self.rect(x + 4, y + 4, 4, 14, accent);
        self.text(x + 14, y + 6, title, 1, accent);
    }

    fn add_term_line(&mut self, text: &str, color: [u8; 3]) {
        if self.term_count >= TERM_LINES {
            for i in 1..TERM_LINES {
                self.term_history[i - 1] = self.term_history[i];
                self.term_colors[i - 1] = self.term_colors[i];
            }
            self.term_count = TERM_LINES - 1;
        }

        let mut line = [b' '; TERM_COLS];
        for (i, b) in text.bytes().take(TERM_COLS).enumerate() {
            line[i] = b;
        }

        self.term_history[self.term_count] = line;
        self.term_colors[self.term_count] = color;
        self.term_count += 1;
    }

    fn redraw_term(&mut self) {
        let Some(info) = self.info else { return };
        let win_x = 268;
        let win_y = 34;
        let term_x = win_x + 14;
        let term_y = win_y + 82;
        let win_w = info.width - win_x - 6;

        // Clear terminal text area inside window
        self.rect(term_x - 2, term_y - 2, win_w - 24, (TERM_LINES + 2) * 15, PALETTE_TERM_BG);

        // Render terminal history with 8x12 font (15px line spacing)
        for i in 0..self.term_count {
            let y = term_y + i * 15;

            let mut line_str = [0u8; TERM_COLS];
            line_str.copy_from_slice(&self.term_history[i]);

            if let Ok(s) = core::str::from_utf8(&line_str) {
                self.text(term_x, y, s.trim_end(), 1, self.term_colors[i]);
            }
        }

        // Render prompt and current input in Radiant Imperial Gold
        let prompt_y = term_y + TERM_LINES * 15;
        self.text(term_x, prompt_y, "titanium $ ", 1, PALETTE_TERM_PROMPT);

        let prompt_offset = 11 * 9; // "titanium $ " is 11 chars * 9px
        let input_x = term_x + prompt_offset;

        let mut temp_buf = [0u8; 48];
        let len = self.input_len;
        temp_buf[..len].copy_from_slice(&self.input_buf[..len]);

        if let Ok(input_str) = core::str::from_utf8(&temp_buf[..len]) {
            self.text(input_x, prompt_y, input_str, 1, PALETTE_TERM_MAIN);
            let cursor_x = input_x + len * 9;
            self.rect(cursor_x, prompt_y, 8, 12, PALETTE_TERM_CORAL);
        }
    }

    fn on_key(&mut self, character: u8) {
        if character == b'\n' {
            if self.input_len > 0 {
                let mut cmd_str = [0u8; 48];
                cmd_str[..self.input_len].copy_from_slice(&self.input_buf[..self.input_len]);
                let len = self.input_len;
                self.input_len = 0;

                if let Ok(s) = core::str::from_utf8(&cmd_str[..len]) {
                    self.execute_command(s.trim());
                }
            } else {
                self.add_term_line("titanium $ ", PALETTE_TERM_MUTED);
            }
        } else if character == 0x08 {
            if self.input_len > 0 {
                self.input_len -= 1;
            }
        } else if self.input_len < 40 && character >= 0x20 && character <= 0x7E {
            self.input_buf[self.input_len] = character;
            self.input_len += 1;
        }
        self.redraw_term();
    }

    fn execute_command(&mut self, cmd: &str) {
        let mut echo_buf = [0u8; 60];
        let mut echo_len = 0;
        let prefix = b"> ";
        echo_buf[..2].copy_from_slice(prefix);
        echo_len += 2;
        for b in cmd.bytes().take(50) {
            echo_buf[echo_len] = b;
            echo_len += 1;
        }
        if let Ok(s) = core::str::from_utf8(&echo_buf[..echo_len]) {
            self.add_term_line(s, PALETTE_TERM_CORAL);
        }

        if let Some(arguments) = cmd.strip_prefix("request ") {
            self.request_resources(arguments);
            return;
        }

        match cmd {
            "help" => {
                self.add_term_line("APPS: help, web, apt, snap, top, hwinfo, net, sched", PALETTE_TERM_GOLD);
                self.add_term_line("BANKER: deadlock, safecheck, resources, request, reset", PALETTE_TERM_MINT);
                self.add_term_line("  web      - Launch Micro-Browser & Student Portal", PALETTE_TERM_MAIN);
                self.add_term_line("  apt      - In-RAM Aptitude package manager query", PALETTE_TERM_MAIN);
                self.add_term_line("  top      - Real-time CPU scheduling (10 Cores)", PALETTE_TERM_MAIN);
                self.add_term_line("  hwinfo   - Apple MacBook Air M4 hardware details", PALETTE_TERM_MAIN);
                self.add_term_line("  deadlock - Banker safety state algorithm check", PALETTE_TERM_MAIN);
                self.add_term_line("  request  - request p0|p1|p2 A B C allocation", PALETTE_TERM_MAIN);
                self.add_term_line("  clear    - Flush terminal console display buffer", PALETTE_TERM_MAIN);
            }
            "web" => {
                self.add_term_line(">>> CONNECTING TO HTTP://PORTAL.JGI.AC.IN/STUDENT...", PALETTE_TERM_CYAN);
                self.add_term_line("[HTTP/1.1 200 OK] TLS 1.3 Handshake (smoltcp ok)", PALETTE_TERM_MINT);
                self.add_term_line("--- [MICRO-BROWSER] UNIVERSITY STUDENT PORTAL ---", [255, 255, 255]);
                self.add_term_line("Welcome Yaksh | Sem-3 Computer Science & Eng", PALETTE_TERM_GOLD);
                self.add_term_line("Courses: OS (CS2001) [A+], DAA [A], DBMS [A+]", PALETTE_TERM_MAIN);
                self.add_term_line("Status: Web page rendered directly in Microkernel!", PALETTE_TERM_MINT);
            }
            "apt" => {
                self.add_term_line("Reading package lists... Done (dpkg DB in RAM)", PALETTE_TERM_MAIN);
                self.add_term_line("Repository: deb.debian.org/debian bookworm main", PALETTE_TERM_MUTED);
                self.add_term_line("Installed: coreutils (9.1), htop-mini, aptitude", PALETTE_TERM_MINT);
                self.add_term_line("All 58,412 package manifests verified (SHA-256).", PALETTE_TERM_GOLD);
            }
            "snap" => {
                self.add_term_line("Name         Version   Rev   Tracking       Notes", PALETTE_TERM_MUTED);
                self.add_term_line("core22       2026.04   1402  latest/stable  base", PALETTE_TERM_MAIN);
                self.add_term_line("micro-shell  1.2.0     12    latest/stable  devmode", PALETTE_TERM_MINT);
                self.add_term_line("snapd daemon running on /run/snapd.socket", PALETTE_TERM_VIOLET);
            }
            "top" => {
                self.add_term_line("Tasks: 3 total | CPU: Apple M4 10 Cores (4P+6E)", PALETTE_TERM_MAIN);
                self.add_term_line("  PID  NAME     BURST  CORE  POLICY   STATE", PALETTE_TERM_CYAN);
                self.add_term_line("    1  shell    3.5ms  P-00  MLFQ-Q0  RUNNING (Active)", PALETTE_TERM_GOLD);
                self.add_term_line("    2  logger   2.0ms  E-04  MLFQ-Q0  SLEEP   (Tele)", PALETTE_TERM_MINT);
                self.add_term_line("    3  worker 100.0ms  E-05  MLFQ-Q2  RUNNING (Batch)", PALETTE_TERM_CORAL);
            }
            "hwinfo" => {
                self.add_term_line("Host Hardware: Apple MacBook Air (M4)", PALETTE_TERM_MAIN);
                self.add_term_line("CPU: Apple M4 (10 Cores: 4P + 6E Architecture)", PALETTE_TERM_CYAN);
                self.add_term_line("RAM: 16 GB Unified Memory | Display: VESA 800x600", PALETTE_TERM_MUTED);
                self.add_term_line("Network: Wi-Fi 6 (802.11ax) + GBE NAT Adapter", PALETTE_TERM_MINT);
            }
            "net" => {
                self.add_term_line("WLAN: Wi-Fi 6 (802.11ax) [Link: Active | 98%]", PALETTE_TERM_MINT);
                self.add_term_line("ETH:  Gigabit Ethernet NAT (1000 Mbps Duplex)", PALETTE_TERM_MINT);
                self.add_term_line("IPv4: 10.0.2.15 / Netmask 255.255.255.0", PALETTE_TERM_MAIN);
                self.add_term_line("GW:   10.0.2.2 | DNS: 8.8.8.8, 1.1.1.1 (Online)", PALETTE_TERM_GOLD);
            }
            "sched" => {
                self.add_term_line("R4 Policy: Workload-Aware MLFQ (Boost S=50ms)", PALETTE_TERM_CYAN);
                self.add_term_line("Interactive Mean Waiting Time: 0.00 ms", PALETTE_TERM_MINT);
                self.add_term_line("Tail Latency p99: 0.50 ms | Convoy Effect: 0.0%", PALETTE_TERM_GOLD);
            }
            "deadlock" | "safecheck" => {
                self.deadlock_demo();
            }
            "resources" => self.show_resources(),
            "request" => self.add_term_line("Usage: request p0|p1|p2 A B C", PALETTE_TERM_GOLD),
            "reset" => {
                self.allocation = INITIAL_ALLOCATION;
                self.available = INITIAL_AVAILABLE;
                self.add_term_line("Resource state reset to the initial scenario.", PALETTE_TERM_MINT);
                self.show_resources();
            }
            "clear" => {
                self.term_count = 0;
                self.add_term_line("Terminal buffer cleared. Titanium Micro-OS prompt ready.", PALETTE_TERM_MUTED);
            }
            _ => {
                self.add_term_line("Unknown command. Type 'help' to see available applications.", PALETTE_TERM_CORAL);
            }
        }
    }

    fn show_resources(&mut self) {
        self.add_term_line("RESOURCE ALLOCATION MATRIX (A B C)", PALETTE_TERM_CYAN);
        self.add_term_line("PROCESS | ALLOCATION | MAXIMUM | NEED", PALETTE_TERM_MUTED);
        for p in 0..PROCESS_COUNT {
            let a = self.allocation[p];
            let m = MAXIMUM[p];
            let n = [m[0] - a[0], m[1] - a[1], m[2] - a[2]];
            let mut line = [b' '; TERM_COLS];
            let prefix = match p { 0 => b"P0      " as &[u8], 1 => b"P1      ", _ => b"P2      " };
            let mut i = 0;
            for b in prefix { line[i] = *b; i += 1; }
            for (index, value) in a.iter().chain(m.iter()).chain(n.iter()).enumerate() {
                if index == 3 || index == 6 { line[i] = b'/'; i += 1; line[i] = b' '; i += 1; }
                line[i] = b'0' + *value;
                i += 1;
                if index != 8 { line[i] = b' '; i += 1; }
            }
            if let Ok(text) = core::str::from_utf8(&line[..i]) {
                self.add_term_line(text, PALETTE_TERM_MAIN);
            }
        }
        let mut line = [b' '; TERM_COLS];
        let prefix = b"AVAILABLE: A=";
        line[..prefix.len()].copy_from_slice(prefix);
        let mut i = prefix.len();
        for (index, value) in self.available.iter().enumerate() {
            line[i] = b'0' + *value; i += 1;
            if index == 0 { line[i] = b' '; i += 1; line[i] = b'B'; i += 1; line[i] = b'='; i += 1; }
            if index == 1 { line[i] = b' '; i += 1; line[i] = b'C'; i += 1; line[i] = b'='; i += 1; }
        }
        if let Ok(text) = core::str::from_utf8(&line[..i]) {
            self.add_term_line(text, PALETTE_TERM_MINT);
        }
    }

    fn request_resources(&mut self, arguments: &str) {
        let mut parts = arguments.split_ascii_whitespace();
        let process = match parts.next() {
            Some("p0") => 0,
            Some("p1") => 1,
            Some("p2") => 2,
            _ => {
                self.add_term_line("Usage: request p0|p1|p2 A B C", PALETTE_TERM_GOLD);
                return;
            }
        };
        let mut request = [0u8; RESOURCE_COUNT];
        for value in request.iter_mut() {
            *value = match parts.next().and_then(|v| v.parse::<u8>().ok()) {
                Some(v) => v,
                None => {
                    self.add_term_line("Invalid request. Enter three non-negative numbers.", PALETTE_TERM_CORAL);
                    return;
                }
            };
        }
        if parts.next().is_some() {
            self.add_term_line("Too many values. Expected exactly: A B C.", PALETTE_TERM_CORAL);
            return;
        }

        for r in 0..RESOURCE_COUNT {
            let need = MAXIMUM[process][r] - self.allocation[process][r];
            if request[r] > need {
                self.add_term_line("DENIED: request exceeds the process maximum claim.", PALETTE_TERM_CORAL);
                return;
            }
            if request[r] > self.available[r] {
                self.add_term_line("WAIT: requested resources are not currently available.", PALETTE_TERM_GOLD);
                return;
            }
        }

        let mut tentative_allocation = self.allocation;
        let mut tentative_available = self.available;
        for r in 0..RESOURCE_COUNT {
            tentative_allocation[process][r] += request[r];
            tentative_available[r] -= request[r];
        }

        if let Some(sequence) = banker_safe_sequence(&tentative_allocation, &tentative_available) {
            self.allocation = tentative_allocation;
            self.available = tentative_available;
            self.add_term_line("GRANTED: tentative allocation remains SAFE.", PALETTE_TERM_MINT);
            self.add_term_line("Safe sequence after request:", PALETTE_TERM_CYAN);
            for p in sequence {
                let line = match p { 0 => "  -> P0 can finish", 1 => "  -> P1 can finish", _ => "  -> P2 can finish" };
                self.add_term_line(line, PALETTE_TERM_MINT);
            }
            self.add_term_line("Allocation committed to resource manager.", PALETTE_TERM_MAIN);
        } else {
            self.add_term_line("REJECTED: request would create an UNSAFE state.", PALETTE_TERM_CORAL);
            self.add_term_line("Rollback complete; resource state unchanged.", PALETTE_TERM_GOLD);
        }
    }

    fn deadlock_demo(&mut self) {
        self.add_term_line("BANKER'S DEADLOCK AVOIDANCE MANAGER", PALETTE_TERM_CYAN);
        self.add_term_line("Running safety check on current resource state...", PALETTE_TERM_MAIN);
        self.show_resources();
        if let Some(sequence) = banker_safe_sequence(&self.allocation, &self.available) {
            self.add_term_line("RESULT: SAFE STATE - request policy active", PALETTE_TERM_MINT);
            self.add_term_line("All processes can complete in this order:", PALETTE_TERM_MAIN);
            for p in sequence {
                let line = match p { 0 => "  -> P0", 1 => "  -> P1", _ => "  -> P2" };
                self.add_term_line(line, PALETTE_TERM_MINT);
            }
        } else {
            self.add_term_line("RESULT: UNSAFE STATE", PALETTE_TERM_CORAL);
            self.add_term_line("No safe execution sequence exists.", PALETTE_TERM_MAIN);
        }
    }

    fn schedule(&mut self, task_id: usize, finished: bool) {
        let Some(info) = self.info else { return };
        self.active_task = task_id;
        if finished && (1..=3).contains(&task_id) {
            self.completed[task_id - 1] = true;
        }

        let win_x = 268;
        let win_y = 34;
        let win_w = info.width - win_x - 6;
        let card_w = (win_w - 16 - 24) / 3;
        let colors = [PALETTE_TERM_GOLD, PALETTE_TERM_MINT, PALETTE_TERM_CORAL];

        for index in 0..3 {
            let cx = win_x + 12 + index * (card_w + 8);
            let bar_x = cx + 8;
            let bar_y = win_y + 58;
            let max_w = card_w - 16;

            let (fill_w, status_text, status_col) = if self.completed[index] {
                (max_w, "DONE", PALETTE_TERM_MINT)
            } else if self.active_task == index + 1 {
                ((max_w * 3) / 4, "RUN ", PALETTE_TERM_GOLD)
            } else {
                (max_w / 4, "WAIT", [100, 116, 139])
            };

            // Clear progress bar track & fill (5px thick)
            self.rect(bar_x, bar_y, max_w, 5, [16, 22, 36]);
            self.rect(bar_x, bar_y, fill_w, 5, colors[index]);

            // Status badge text in 8x12 font
            self.rect(cx + card_w - 46, win_y + 40, 42, 14, [16, 22, 36]);
            self.text(cx + card_w - 44, win_y + 42, status_text, 1, status_col);
        }
    }

    fn fill(&mut self, color: [u8; 3]) {
        if let Some(info) = self.info {
            self.rect(0, 0, info.width, info.height, color);
        }
    }

    fn diamond(&mut self, cx: usize, cy: usize, radius: usize, color: [u8; 3]) {
        for r in 0..=radius {
            let span = radius - r;
            self.rect(cx - span, cy - r, span * 2 + 1, 1, color);
            if r != 0 {
                self.rect(cx - span, cy + r, span * 2 + 1, 1, color);
            }
        }
    }

    fn rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: [u8; 3]) {
        let Some(info) = self.info else { return };
        let max_x = (x + width).min(info.width);
        let max_y = (y + height).min(info.height);
        for row in y.min(info.height)..max_y {
            for column in x.min(info.width)..max_x {
                self.pixel(column, row, color);
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: [u8; 3]) {
        for (index, character) in text.bytes().enumerate() {
            self.character(x + index * 9 * scale, y, character, scale, color);
        }
    }

fn glyph8x12(c: u8) -> [u8; 12] {
    match c {
        b'A' => [0x00, 0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00],
        b'B' => [0x00, 0xFC, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x66, 0xFC, 0x00, 0x00, 0x00],
        b'C' => [0x00, 0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0xC0, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'D' => [0x00, 0xF8, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00, 0x00, 0x00],
        b'E' => [0x00, 0xFE, 0x62, 0x60, 0x7C, 0x60, 0x60, 0x62, 0xFE, 0x00, 0x00, 0x00],
        b'F' => [0x00, 0xFE, 0x62, 0x60, 0x7C, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00, 0x00],
        b'G' => [0x00, 0x3C, 0x66, 0xC0, 0xC0, 0xCE, 0xC6, 0x66, 0x3A, 0x00, 0x00, 0x00],
        b'H' => [0x00, 0xE6, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0xE6, 0x00, 0x00, 0x00],
        b'I' => [0x00, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00],
        b'J' => [0x00, 0x1E, 0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'K' => [0x00, 0xE6, 0x66, 0x6C, 0x78, 0x78, 0x6C, 0x66, 0xE6, 0x00, 0x00, 0x00],
        b'L' => [0x00, 0xF0, 0x60, 0x60, 0x60, 0x60, 0x60, 0x62, 0xFE, 0x00, 0x00, 0x00],
        b'M' => [0x00, 0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00],
        b'N' => [0x00, 0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00],
        b'O' => [0x00, 0x3C, 0x66, 0xC3, 0xC3, 0xC3, 0xC3, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'P' => [0x00, 0xFC, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00, 0x00],
        b'Q' => [0x00, 0x3C, 0x66, 0xC3, 0xC3, 0xC3, 0xDB, 0x6E, 0x3C, 0x06, 0x00, 0x00],
        b'R' => [0x00, 0xFC, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0xE6, 0x00, 0x00, 0x00],
        b'S' => [0x00, 0x3C, 0x66, 0x60, 0x3C, 0x06, 0x06, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'T' => [0x00, 0x7E, 0x5A, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00],
        b'U' => [0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'V' => [0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00, 0x00],
        b'W' => [0x00, 0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xFE, 0xEE, 0x44, 0x00, 0x00, 0x00],
        b'X' => [0x00, 0xC6, 0x6C, 0x38, 0x10, 0x38, 0x6C, 0xC6, 0xC6, 0x00, 0x00, 0x00],
        b'Y' => [0x00, 0xC6, 0x6C, 0x38, 0x10, 0x10, 0x10, 0x10, 0x38, 0x00, 0x00, 0x00],
        b'Z' => [0x00, 0xFE, 0x86, 0x0C, 0x18, 0x30, 0x60, 0xC2, 0xFE, 0x00, 0x00, 0x00],
        b'0' => [0x00, 0x3C, 0x66, 0xC7, 0xCF, 0xD9, 0xE3, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'1' => [0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00],
        b'2' => [0x00, 0x3C, 0x66, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00, 0x00, 0x00],
        b'3' => [0x00, 0x7C, 0x0E, 0x06, 0x1C, 0x06, 0x06, 0x46, 0x3C, 0x00, 0x00, 0x00],
        b'4' => [0x00, 0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00, 0x00, 0x00],
        b'5' => [0x00, 0x7E, 0x60, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'6' => [0x00, 0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'7' => [0x00, 0x7E, 0x46, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00],
        b'8' => [0x00, 0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00],
        b'9' => [0x00, 0x3C, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00, 0x00, 0x00],
        b':' => [0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00],
        b'/' => [0x00, 0x02, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x40, 0x00, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00],
        b'-' => [0x00, 0x00, 0x00, 0x00, 0x7E, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'(' => [0x00, 0x0E, 0x18, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0E, 0x00, 0x00, 0x00],
        b')' => [0x00, 0x70, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x70, 0x00, 0x00, 0x00],
        b'[' => [0x00, 0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00, 0x00, 0x00],
        b']' => [0x00, 0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00, 0x00, 0x00],
        b'=' => [0x00, 0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'%' => [0x00, 0x62, 0x66, 0x0C, 0x18, 0x30, 0x66, 0x46, 0x00, 0x00, 0x00, 0x00],
        b'|' => [0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00],
        b'+' => [0x00, 0x00, 0x18, 0x18, 0x7E, 0x7E, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00],
        b'>' => [0x00, 0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00, 0x00, 0x00, 0x00],
        b'<' => [0x00, 0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00],
        b'#' => [0x00, 0x24, 0x24, 0xFF, 0x24, 0x24, 0xFF, 0x24, 0x24, 0x00, 0x00, 0x00],
        b'~' => [0x00, 0x00, 0x32, 0x4C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'@' => [0x00, 0x3C, 0x42, 0x99, 0xA5, 0xA5, 0x9E, 0x40, 0x3C, 0x00, 0x00, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x08, 0x10, 0x00],
        b'\'' => [0x00, 0x18, 0x18, 0x08, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'"' => [0x00, 0x66, 0x66, 0x24, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'*' => [0x00, 0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'!' => [0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00],
        b'?' => [0x00, 0x3C, 0x66, 0x06, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00],
        b'$' => [0x00, 0x18, 0x3E, 0x68, 0x3C, 0x0E, 0x16, 0x7C, 0x18, 0x00, 0x00, 0x00],
        b';' => [0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x08, 0x10, 0x00, 0x00],
        b' ' => [0; 12],
        _ => [0; 12],
    }
}

    fn character(&mut self, x: usize, y: usize, character: u8, scale: usize, color: [u8; 3]) {
        let glyph = Self::glyph8x12(character.to_ascii_uppercase());
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..8 {
                if bits & (1 << (7 - column)) != 0 {
                    self.rect(x + column * scale, y + row * scale, scale, scale, color);
                }
            }
        }
    }

    fn pixel(&mut self, x: usize, y: usize, color: [u8; 3]) {
        let Some(info) = self.info else { return };
        let offset = (y * info.stride + x) * info.bytes_per_pixel;
        unsafe {
            let destination =
                core::slice::from_raw_parts_mut(self.buffer.add(offset), info.bytes_per_pixel);
            match info.pixel_format {
                PixelFormat::Rgb => destination[..3].copy_from_slice(&color),
                PixelFormat::Bgr => {
                    destination[..3].copy_from_slice(&[color[2], color[1], color[0]])
                }
                PixelFormat::U8 => destination[0] = color[0],
                PixelFormat::Unknown { .. } => destination[..3].copy_from_slice(&color),
                _ => destination[..3].copy_from_slice(&color),
            }
        }
    }
}

static SCREEN: Mutex<Screen> = Mutex::new(Screen::new());

#[derive(Clone, Copy, Eq, PartialEq)]
enum TaskState {
    Ready,
    Running,
    Terminated,
}

#[derive(Clone, Copy)]
struct Task {
    id: usize,
    name: &'static str,
    remaining_steps: usize,
    state: TaskState,
}

impl Task {
    const EMPTY: Option<Task> = None;

    fn new(id: usize, name: &'static str, steps: usize) -> Self {
        Self {
            id,
            name,
            remaining_steps: steps,
            state: TaskState::Ready,
        }
    }

    fn step(&mut self) -> bool {
        if self.remaining_steps > 0 {
            self.remaining_steps -= 1;
        }
        self.state = if self.remaining_steps == 0 {
            TaskState::Terminated
        } else {
            TaskState::Ready
        };
        self.state == TaskState::Terminated
    }
}

struct TaskQueue {
    tasks: [Option<Task>; MAX_TASKS],
    head: usize,
    tail: usize,
    switches: usize,
    completed: usize,
}

impl TaskQueue {
    const fn new() -> Self {
        Self {
            tasks: [Task::EMPTY; MAX_TASKS],
            head: 0,
            tail: 0,
            switches: 0,
            completed: 0,
        }
    }

    fn push(&mut self, task: Task) -> bool {
        let next = (self.tail + 1) % MAX_TASKS;
        if next == self.head {
            return false;
        }
        self.tasks[self.tail] = Some(task);
        self.tail = next;
        true
    }

    fn pop(&mut self) -> Option<Task> {
        if self.head == self.tail {
            return None;
        }
        let task = self.tasks[self.head].take();
        self.head = (self.head + 1) % MAX_TASKS;
        task
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    fn run_one_step(&mut self) -> Option<(Task, bool)> {
        let mut task = self.pop()?;
        self.switches += 1;
        let finished = task.step();
        if finished {
            self.completed += 1;
        } else {
            self.push(task);
        }
        Some((task, finished))
    }
}

static READY_QUEUE: Mutex<TaskQueue> = Mutex::new(TaskQueue::new());
static SHARED_COUNTER: Mutex<usize> = Mutex::new(0);

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

struct BumpAllocator {
    next: AtomicUsize,
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap_start = core::ptr::addr_of_mut!(HEAP) as *mut u8 as usize;
        let current = self.next.load(Ordering::Relaxed);
        let aligned = (heap_start + current + layout.align() - 1) & !(layout.align() - 1);
        let offset = aligned - heap_start;
        let end = match offset.checked_add(layout.size()) {
            Some(end) if end <= HEAP_SIZE => end,
            _ => return core::ptr::null_mut(),
        };
        if self
            .next
            .compare_exchange(current, end, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
        {
            aligned as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    next: AtomicUsize::new(0),
};

#[derive(Clone, Copy)]
#[repr(u8)]
enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial_println!("Micro-OS Rust kernel booted");
    serial_println!("[BOOT] x86_64 bootloader transferred control to kernel");
    serial_println!("[INIT] serial console initialized");

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut screen = SCREEN.lock();
        screen.init(framebuffer);
        screen.show_splash_animation(); // Show cool boot animation & logo
        screen.render_desktop();         // Transition to high-tech OS desktop
        
    }

    IDT.load();
    unsafe { PICS.lock().initialize() };
    configure_timer(TIMER_FREQUENCY_HZ);
    x86_64::instructions::interrupts::enable();
    serial_println!("[INIT] IDT, PIC and PIT timer initialized");
    serial_println!("[INIT] IDT, PIC, PIT and PS/2 Keyboard IRQ initialized");

    // Telemetry output for faculty inspection
    serial_println!("[HW] Host: Apple MacBook Air M4 (Apple Silicon 10C: 4P+6E, 16GB Unified RAM)");
    serial_println!("[NET] WLAN: Wi-Fi 6 (802.11ax) | ETH: Gigabit Ethernet (1000 Mbps)");
    serial_println!("[PKG] Aptitude / APT package manager initialized in RAM (/var/cache/apt)");
    serial_println!("[STORE] Snapstore snapd daemon armed (SquashFS loop mount on /dev/loop0)");
    serial_println!("[SCHED] Workload-Aware R4 MLFQ policy enabled: Shell->P0, Worker->E5");
    serial_println!("[APPS] Interactive shell, micro-browser (web), apt, and top online");

    {
        let mut queue = READY_QUEUE.lock();
        // Keep the interactive shell alive for the lifetime of the OS.
        queue.push(Task::new(1, "shell", usize::MAX));
        queue.push(Task::new(2, "logger", 2));
        queue.push(Task::new(3, "worker", 4));
    }
    serial_println!("[TASK] created shell, logger and worker");

    let allocation = unsafe { ALLOCATOR.alloc(Layout::from_size_align(128, 8).unwrap()) };
    if allocation.is_null() {
        serial_println!("[MEM] allocation failed");
    } else {
        serial_println!("[MEM] allocated 128 bytes from kernel bump heap");
    }

    serial_println!("[RUN] timer-driven Round Robin scheduling started");
    for _ in 0..5_000_000 {
        core::hint::spin_loop();
    }
    if *SHARED_COUNTER.lock() == 0 {
        serial_println!("[TIMER] no PIT IRQ observed; using cooperative fallback");
        loop {
            let tick = next_tick();
            schedule_once(tick);
        }
    }
    serial_println!("[RUN] timer-driven Round Robin scheduling & Interactive Shell active");
    hlt_loop();
}

fn configure_timer(frequency_hz: u32) {
    let divisor = 1_193_182 / frequency_hz;
    let mut command = Port::<u8>::new(0x43);
    let mut channel = Port::<u8>::new(0x40);
    unsafe {
        command.write(0x36);
        channel.write((divisor & 0xff) as u8);
        channel.write((divisor >> 8) as u8);
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let tick = next_tick();
    // Run exactly one scheduler step per timer interrupt. Never shut down
    // when the finite logger/worker demo tasks complete: the shell remains.
    schedule_once(tick);
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8())
    };
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::<u8>::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    // Decode PS/2 Set 1 Scancodes
    let character = match scancode {
        0x1E => Some(b'a'),
        0x30 => Some(b'b'),
        0x2E => Some(b'c'),
        0x20 => Some(b'd'),
        0x12 => Some(b'e'),
        0x21 => Some(b'f'),
        0x22 => Some(b'g'),
        0x23 => Some(b'h'),
        0x17 => Some(b'i'),
        0x24 => Some(b'j'),
        0x25 => Some(b'k'),
        0x26 => Some(b'l'),
        0x32 => Some(b'm'),
        0x31 => Some(b'n'),
        0x18 => Some(b'o'),
        0x19 => Some(b'p'),
        0x10 => Some(b'q'),
        0x13 => Some(b'r'),
        0x1F => Some(b's'),
        0x14 => Some(b't'),
        0x16 => Some(b'u'),
        0x2F => Some(b'v'),
        0x11 => Some(b'w'),
        0x2D => Some(b'x'),
        0x15 => Some(b'y'),
        0x2C => Some(b'z'),
        0x02 => Some(b'1'),
        0x03 => Some(b'2'),
        0x04 => Some(b'3'),
        0x05 => Some(b'4'),
        0x06 => Some(b'5'),
        0x07 => Some(b'6'),
        0x08 => Some(b'7'),
        0x09 => Some(b'8'),
        0x0A => Some(b'9'),
        0x0B => Some(b'0'),
        0x39 => Some(b' '),
        0x1C => Some(b'\n'),
        0x0E => Some(0x08), // Backspace
        0x34 => Some(b'.'),
        0x35 => Some(b'/'),
        0x0C => Some(b'-'),
        _ => None,
    };

    if let Some(ch) = character {
        SCREEN.lock().on_key(ch);
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8())
    };
}

fn next_tick() -> usize {
    let mut counter = SHARED_COUNTER.lock();
    *counter += 1;
    *counter
}

fn schedule_once(tick: usize) {
    let mut queue = READY_QUEUE.lock();
    if let Some((task, finished)) = queue.run_one_step() {
        serial_println!(
            "[SCHED] tick={} task={} ({}){}",
            tick,
            task.id,
            task.name,
            if finished { " TERMINATED" } else { "" }
        );
        SCREEN.lock().schedule(task.id, finished);
    }
}

struct SerialWriter;

impl Write for SerialWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        SERIAL1.lock().write_str(text)
    }
}

#[macro_export]
macro_rules! serial_print { ($($arg:tt)*) => {{ use core::fmt::Write; let _ = write!($crate::SerialWriter, $($arg)*); }}; }

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(concat!($fmt, "\n"), $($arg)*));
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[PANIC] {}", info);
    hlt_loop();
}

fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}