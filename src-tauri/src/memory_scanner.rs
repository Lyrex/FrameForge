#[cfg(target_os = "linux")]
use aho_corasick::AhoCorasick;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::fs::FileExt;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModCount {
    /// Total copies owned (all ranks combined)
    pub total: i64,
    /// rank (0 = unranked) → count at that rank
    pub by_rank: HashMap<u8, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlobRivenStat {
    pub tag:   String,
    pub value: i64,
}

/// Which stage of unlocking a riven is at. Matches warframe.market terminology.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RivenState {
    /// From RawUpgrades — only the weapon type (Rifle/Pistol/Melee…) is visible.
    Unrevealed,
    /// From Upgrades with a `challenge` fingerprint but no `compat` — challenge is visible
    /// but not yet completed; weapon has not been assigned.
    Revealed,
    /// From Upgrades with a `compat` — weapon assigned, stats fully visible.
    #[default]
    Unlocked,
}

/// One owned riven mod (unrevealed, revealed, or fully unlocked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRivenEntry {
    /// MongoDB ObjectId hex string (empty for unrevealed stacks).
    pub item_id:  String,
    /// Lotus path, e.g. /Lotus/Upgrades/Mods/Randomized/LotusMeleeRandomModRare
    pub item_type: String,
    /// Which stage this riven is at (unrevealed / revealed / unlocked).
    /// Old cache entries without this field default to Unlocked.
    #[serde(default)]
    pub riven_state: RivenState,
    /// Weapon unique_name from `compat` field. Only present for Unlocked rivens.
    pub compat:   Option<String>,
    /// Challenge path from fingerprint. Only present for Revealed rivens.
    /// e.g. "/Lotus/Types/Challenges/HighExterminationUndetected"
    #[serde(default)]
    pub challenge_type: Option<String>,
    /// Complication path. e.g. "/Lotus/Types/Challenges/Complications/SoloPlayer"
    #[serde(default)]
    pub challenge_complication: Option<String>,
    /// MR required to equip.
    pub lvl_req:  Option<u32>,
    /// Polarity slot (AP_ATTACK, AP_DEFENSE, etc.).
    pub polarity: Option<String>,
    pub buffs:    Vec<BlobRivenStat>,
    pub curses:   Vec<BlobRivenStat>,
    /// Current mod level (rank).
    pub mod_rank: u8,
    /// >1 for stacked unrevealed rivens of the same type.
    pub count:    u32,
    /// Number of times this riven has been re-rolled (Kuva spent). 0 = never rolled.
    #[serde(default)]
    pub rerolls:  u32,
    /// Generated riven mod name (e.g. "cronitron"). Computed from buffs at parse time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mod_name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PendingRecipe {
    pub unique_name: String,
    /// Unix timestamp in milliseconds when the craft completes
    pub completion_ms: i64,
}

/// One Archon Shard socketed into a Warframe.
/// One Archon Shard socketed into a Warframe.
/// `upgrade_type` is the effect path (e.g. `.../ArchonCrystalUpgradeWarframeEnergyMax`).
/// `color` is the raw string value from the JSON (e.g. `"ACC_CRIMSON"`, `"ACC_AZURE_TAUFORGED"`).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchonShard {
    pub upgrade_type: String,
    pub color: String,
}

// ─── Blob inventory types ─────────────────────────────────────────────────────

/// Parsed representation of an Actual_inventory_FULL_ACCOUNT blob.
/// Single authoritative source for all inventory data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlobInventory {
    pub credits:         i64,
    pub endo:            i64,
    pub platinum:        i64,
    pub free_platinum:   i64,
    pub mastery_level:   u32,
    pub unique_items:    Vec<BlobUniqueEntry>,
    pub stackable_items: Vec<BlobStackableEntry>,
    /// Aggregated from RawUpgrades (unranked) + Upgrades (ranked).
    pub mods:            HashMap<String, ModCount>,
    /// FlavourItems — glyphs, palettes, emotes, titles, ship skins. Path → occurrence count.
    pub flavour_items:   HashMap<String, i64>,
    /// WeaponSkins — sigils and cosmetic weapon overlays. Path → occurrence count.
    pub weapon_skins:    HashMap<String, i64>,
    /// Path → mastery rank derived from XPInfo.
    pub mastery_data:    HashMap<String, u32>,
    pub pending_recipes: Vec<BlobPendingRecipe>,
    /// Warframe paths fed to Helminth (InfestedFoundry.ConsumedSuits).
    pub consumed_suits:  Vec<String>,
    /// All owned riven mods (veiled and revealed).
    pub rivens:          Vec<BlobRivenEntry>,
}

/// One owned unique item (warframe, weapon, companion, archwing, amp, mech).
/// Multiple entries with the same item_type = multiple owned copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobUniqueEntry {
    pub item_type:     String,
    pub section:       String,
    pub polarized:     u32,
    pub pet_name:      Option<String>,
    pub focus_lens:    Option<String>,
    pub archon_shards: Vec<ArchonShard>,
}

/// A stackable item: resource, blueprint, relic, Ayatan sculpture, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobStackableEntry {
    pub item_type:  String,
    pub item_count: i64,
    /// Ayatan sockets (FusionTreasures only).
    pub sockets:    Option<i64>,
}

/// Active Foundry crafting job parsed from PendingRecipes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobPendingRecipe {
    pub item_type:     String,
    pub completion_ms: i64,
}

fn digits_end(data: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < data.len() && data[i].is_ascii_digit() { i += 1; }
    i
}

/// Convert raw affinity XP to item rank (0–30).
/// Formula from Warframe wiki: cumulative XP to reach rank N is 1000×N² for
/// Warframes/Sentinels/companions, 500×N² for all weapon types.
/// Invert: rank = floor(sqrt(xp / base)).
pub fn xp_to_rank(xp: i64, path: &str) -> u32 {
    let base = if path.contains("/Powersuits/")
        || path.contains("/SentinelPowersuits/")
        || path.contains("/Types/Friendly/")
        || path.contains("/Types/Game/KubrowPet/")
        || path.contains("/Types/Game/CatbrowPet/")
    { 1000.0f64 } else { 500.0f64 };
    ((xp as f64 / base).sqrt().floor() as u32).min(30)
}

/// Diagnostic: find "CompletionDate" in any format and return a snippet of context.
#[allow(dead_code)]
pub fn scan_completion_date_context(data: &[u8]) -> Vec<String> {
    let key = b"\"CompletionDate\"";
    let mut results = Vec::new();
    let mut start = 0usize;
    loop {
        let next = match data[start..].iter().position(|&b| b == b'"') {
            Some(p) => start + p,
            None => break,
        };
        if next + key.len() > data.len() { break; }
        if data[next..next + key.len()] != *key {
            start = next + 1; continue;
        }
        // Capture 120 bytes of context starting 40 bytes before the key
        let ctx_start = next.saturating_sub(40);
        let ctx_end   = (next + 120).min(data.len());
        let ctx = &data[ctx_start..ctx_end];
        // Only include printable ASCII so the log is readable
        let s: String = ctx.iter()
            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
            .collect();
        results.push(s);
        start = next + key.len();
        if results.len() >= 3 { break; } // cap at 3 samples
    }
    results
}


// ─── Auth credentials scan ───────────────────────────────────────────────────
//
// When Warframe is running and logged in, the game stores the session credentials
// in memory as URL-encoded strings: accountId=<id>&nonce=<nonce>
// We scan for these to authenticate with the Warframe companion API.

pub fn scan_auth_credentials(data: &[u8]) -> Option<(String, String)> {
    // The Warframe game receives a login response JSON from DE's servers containing:
    //   {"id":"<24-char-hex-accountId>","Nonce":<large-integer>,...}
    // We search for this pattern. The Nonce is typically 9-13 digits.
    // We also try URL-encoded form: accountId=<id>&nonce=<nonce>
    //
    // Key insight from devtools: accountId=594144e63ade7f2f2091c48e (24ch), nonce len=9
    // The 24-char hex accountId is a MongoDB ObjectId — correct format.
    // The 9-digit nonce IS valid — it's a server-issued integer session token.

    // Search for "id":"<24hexchars>" near "Nonce":<digits>
    let id_key = b"\"id\":\"";
    let nonce_key = b"\"Nonce\":";
    let mut search = 0usize;
    while search + id_key.len() < data.len() {
        let next = match data[search..].iter().position(|&b| b == b'"') {
            Some(p) => search + p, None => break,
        };
        if next + id_key.len() > data.len() { break; }
        if data[next..next + id_key.len()] != *id_key { search = next + 1; continue; }

        let id_start = next + id_key.len();
        // accountId is exactly 24 lowercase hex chars
        let id_slice = &data[id_start..id_start.saturating_add(26).min(data.len())];
        let close = id_slice.iter().position(|&b| b == b'"').unwrap_or(0);
        if close != 24 { search = next + 1; continue; }
        let id_bytes = &id_slice[..24];
        if !id_bytes.iter().all(|&b| b.is_ascii_hexdigit()) { search = next + 1; continue; }
        let account_id = std::str::from_utf8(id_bytes).unwrap_or("").to_string();

        // Look for Nonce within 2048 bytes
        let nonce_search_end = (id_start + 2048).min(data.len());
        if let Some(rel) = data[id_start..nonce_search_end].windows(nonce_key.len()).position(|w| w == *nonce_key) {
            let ns = id_start + rel + nonce_key.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
        search = next + 1;
    }

    // URL-encoded: accountId=<24hexchars>&nonce=<10digits>&ct=STM
    let ak = b"accountId=";
    let nk = b"nonce=";
    let mut search = 0usize;
    while search + ak.len() < data.len() {
        let next = match data[search..].iter().position(|&b| b == b'a') {
            Some(p) => search + p, None => break,
        };
        if next + ak.len() > data.len() { break; }
        if data[next..next + ak.len()] != *ak { search = next + 1; continue; }
        let id_start = next + ak.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_hexdigit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start != 24 { search = next + 1; continue; }
        let account_id = std::str::from_utf8(&data[id_start..id_end]).unwrap_or("").to_string();
        // Nonce can appear anywhere within 512 bytes after the accountId
        let nonce_search_end = (id_end + 512).min(data.len());
        if let Some(rel) = data[id_end..nonce_search_end].windows(nk.len()).position(|w| w == *nk) {
            let ns = id_end + rel + nk.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
        search = next + 1;
    }
    None
}

/// Also extract steamId from memory (found near accountId/nonce in URL params).
pub fn scan_steam_id(data: &[u8]) -> Option<String> {
    let key = b"steamId=";
    let mut search = 0usize;
    loop {
        let next = match data[search..].iter().position(|&b| b == b's') {
            Some(p) => search + p, None => break,
        };
        if next + key.len() > data.len() { break; }
        if data[next..next + key.len()] != *key { search = next + 1; continue; }
        let id_start = next + key.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_digit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start >= 15 && id_end - id_start <= 20 {
            if let Ok(sid) = std::str::from_utf8(&data[id_start..id_end]) {
                return Some(sid.to_string());
            }
        }
        search = next + 1;
    }
    None
}

// ─── Public helpers ──────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn find_warframe_pid_pub() -> Option<u32> { find_warframe_pid() }

#[cfg(target_os = "linux")]
pub fn find_warframe_pid_pub() -> Option<u32> { find_warframe_pid() }

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn find_warframe_pid_pub() -> Option<u32> { None }

#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
struct LinuxRegion {
    start: usize,
    len: usize,
    executable: bool,
}

#[cfg(target_os = "linux")]
struct LinuxProcess {
    memory: File,
}

#[cfg(target_os = "linux")]
impl LinuxProcess {
    fn open(pid: u32) -> Result<Self, String> {
        open_linux_process_memory(pid).map(|memory| Self { memory })
    }

    fn read(&self, address: usize, buffer: &mut [u8]) -> std::io::Result<usize> {
        read_linux_process_memory(&self.memory, address, buffer)
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_maps(maps: &str) -> Vec<LinuxRegion> {
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (start, end) = fields.next()?.split_once('-')?;
            let permissions = fields.next()?;
            if !permissions.starts_with('r') {
                return None;
            }
            let start = usize::from_str_radix(start, 16).ok()?;
            let end = usize::from_str_radix(end, 16).ok()?;
            Some(LinuxRegion {
                start,
                len: end.checked_sub(start)?,
                executable: permissions.as_bytes().get(2) == Some(&b'x'),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_process_regions(pid: u32) -> Result<Vec<LinuxRegion>, String> {
    let path = format!("/proc/{pid}/maps");
    std::fs::read_to_string(&path)
        .map(|maps| parse_linux_maps(&maps))
        .map_err(|error| {
            format!(
                "Failed to read {path}: {error}. Ensure kernel.yama.ptrace_scope permits same-user process access"
            )
        })
}

#[cfg(target_os = "linux")]
fn open_linux_process_memory(pid: u32) -> Result<File, String> {
    let path = format!("/proc/{pid}/mem");
    File::open(&path).map_err(|error| {
        format!(
            "Failed to open {path}: {error}. Ensure kernel.yama.ptrace_scope permits same-user process access"
        )
    })
}

#[cfg(target_os = "linux")]
fn read_linux_process_memory(
    memory: &File,
    address: usize,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    memory.read_at(buffer, address as u64)
}

#[cfg(target_os = "windows")]
pub fn scan_warframe_credentials_process() -> Result<(String, String, String), String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = find_warframe_pid_pub().ok_or("Warframe is not running")?;

    unsafe {
        let process = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid);
        if process == 0 { return Err("Cannot open Warframe process".into()); }

        let mut address: usize = 0x10000;
        let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();

        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(process, address as *const c_void, &mut mbi, mbi_size) == 0 { break; }
            let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
            if region_end <= address { break; }
            address = region_end;

            if mbi.State != MEM_COMMIT { continue; }
            let protection = mbi.Protect;
            if protection & PAGE_NOACCESS != 0 || protection & PAGE_GUARD != 0 { continue; }
            if protection == 0x10 || protection == 0x20 { continue; }
            if mbi.RegionSize > 128 * 1024 * 1024 { continue; }

            let mut buffer = vec![0u8; mbi.RegionSize];
            let mut bytes_read: usize = 0;
            let ok = ReadProcessMemory(
                process,
                mbi.BaseAddress as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                mbi.RegionSize,
                &mut bytes_read,
            );
            if ok == 0 || bytes_read == 0 { continue; }

            let data = &buffer[..bytes_read];
            if let Some((id, nonce)) = scan_auth_credentials(data) {
                let steam_id = scan_steam_id(data).unwrap_or_default();
                CloseHandle(process);
                return Ok((id, nonce, steam_id));
            }
        }
        CloseHandle(process);
    }
    Err("Credentials not found in memory. Make sure you are in the orbiter (not loading screen) and Warframe has been running for a few minutes.".into())
}

#[cfg(target_os = "linux")]
pub fn scan_warframe_credentials_process() -> Result<(String, String, String), String> {
    let pid = find_warframe_pid_pub().ok_or("Warframe is not running")?;
    let process = LinuxProcess::open(pid)?;
    let regions = linux_process_regions(pid)?;

    scan_linux_credential_regions(&process, regions).ok_or_else(|| {
        "Credentials not found in memory. Make sure you are in the orbiter (not loading screen) \
         and Warframe has been running for a few minutes."
            .into()
    })
}

/// Walk readable data mappings looking for the login response the game keeps in
/// memory. Split from the command above so it can be exercised against the test
/// process, where the mappings and the expected bytes are both known.
#[cfg(target_os = "linux")]
fn scan_linux_credential_regions(
    process: &LinuxProcess,
    regions: impl IntoIterator<Item = LinuxRegion>,
) -> Option<(String, String, String)> {
    // Matches the Windows scanner's per-region ceiling: anything larger is a
    // texture or heap arena, not the small JSON blob we are after, and reading
    // it would cost hundreds of megabytes of copies per scan.
    const MAX_REGION: usize = 128 * 1024 * 1024;

    for region in regions {
        if region.executable || region.len > MAX_REGION {
            continue;
        }
        let mut buffer = vec![0; region.len];
        let read = match process.read(region.start, &mut buffer) {
            Ok(read) if read > 0 => read,
            Ok(_) | Err(_) => continue,
        };
        let data = &buffer[..read];
        if let Some((id, nonce)) = scan_auth_credentials(data) {
            return Some((id, nonce, scan_steam_id(data).unwrap_or_default()));
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn scan_warframe_credentials_process() -> Result<(String, String, String), String> {
    Err("Only supported on Windows and Linux".into())
}

#[cfg(target_os = "linux")]
fn is_warframe_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("warframe.x64.exe")
        && !command.contains("launcher.exe")
        && !command.contains("warframe-companion")
}

// ─── Raw memory format probe ──────────────────────────────────────────────────
//
// Scans Warframe's memory and returns raw text context around every occurrence
// of a set of known strings.  Capped at max_hits total.  Used to reverse-engineer
// the actual JSON format for inventory items without any parsing assumptions.

#[cfg(target_os = "windows")]
pub fn dump_inventory_regions(max_hits: usize) -> Vec<String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    // Patterns to search for — ordered by diagnostic value.
    // "MiscItems":[{ marks the beginning of the actual inventory JSON array from DE's API
    // response (the most useful single needle for finding the real JSON blob).
    const NEEDLES: &[&[u8]] = &[
        b"\"MiscItems\":[{",      // inventory JSON array start — best diagnostic
        b"\"ItemCount\":",
        b"MiscItems",
        b"AlloyPlate",
        b"Circuits\"",
        b"/Lotus/Types/Items/MiscItems/",
    ];

    let pid = match find_warframe_pid() {
        Some(p) => p,
        None => return vec!["Warframe not running".to_string()],
    };

    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return vec!["OpenProcess failed".to_string()]; }

    let mut results: Vec<String> = Vec::new();
    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    'outer: while std::time::Instant::now() < deadline && results.len() < max_hits {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        if mbi.State != MEM_COMMIT { continue; }
        let p = mbi.Protect;
        if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
        if p == 0x10 || p == 0x20 { continue; }    // skip executable (code) pages
        // Skip tiny or enormous regions; read large regions in 64 MB chunks
        const MAX_REGION: usize = 256 * 1024 * 1024;
        const CHUNK_SIZE: usize =  64 * 1024 * 1024;
        if mbi.RegionSize < 4096 || mbi.RegionSize > MAX_REGION { continue; }

        let chunks = if mbi.RegionSize > CHUNK_SIZE {
            (mbi.RegionSize + CHUNK_SIZE - 1) / CHUNK_SIZE
        } else { 1 };

        'chunk: for chunk_idx in 0..chunks {
            if results.len() >= max_hits { break 'outer; }
            if std::time::Instant::now() >= deadline { break 'outer; }

            let chunk_offset = chunk_idx * CHUNK_SIZE;
            let read_size    = CHUNK_SIZE.min(mbi.RegionSize - chunk_offset);
            let chunk_addr   = mbi.BaseAddress as usize + chunk_offset;

            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = unsafe {
                ReadProcessMemory(process, chunk_addr as *const c_void,
                    buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read)
            };
            if ok == 0 || bytes_read < 8 { continue 'chunk; }
            let data = &buf[..bytes_read];

        for needle in NEEDLES {
            if results.len() >= max_hits { break 'outer; }
            if let Some(pos) = data.windows(needle.len()).position(|w| w == *needle) {
                let ctx_start = pos.saturating_sub(80);
                let ctx_end   = data.len().min(pos + 200);
                let snip: String = data[ctx_start..ctx_end].iter()
                    .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
                    .collect();
                results.push(format!(
                    "0x{:012x}  needle=\"{}\"  ctx: {}",
                    chunk_addr + ctx_start,
                    String::from_utf8_lossy(needle),
                    snip
                ));
                // Also grab up to 2 more occurrences of the same needle in this chunk
                let mut search = pos + needle.len();
                let mut extra = 0;
                while extra < 2 && search + needle.len() <= data.len() {
                    if let Some(rel) = data[search..].windows(needle.len()).position(|w| w == *needle) {
                        let p2 = search + rel;
                        let s2 = p2.saturating_sub(80);
                        let e2 = data.len().min(p2 + 200);
                        let snip2: String = data[s2..e2].iter()
                            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
                            .collect();
                        results.push(format!(
                            "0x{:012x}  needle=\"{}\"  ctx: {}",
                            chunk_addr + s2,
                            String::from_utf8_lossy(needle),
                            snip2
                        ));
                        search = p2 + needle.len();
                        extra += 1;
                    } else { break; }
                }
            }
        }
        } // end 'chunk loop
    }

    unsafe { CloseHandle(process); }
    if results.is_empty() { results.push("No matches found".to_string()); }
    results
}

#[cfg(not(target_os = "windows"))]
pub fn dump_inventory_regions(_max_hits: usize) -> Vec<String> {
    vec!["Only supported on Windows".to_string()]
}

// ─── One-shot inventory blob capture ─────────────────────────────────────────
//
// Scans all committed readable regions for the first chunk that contains the
// inventory root marker ("MiscItems":[).  Saves the full printable-text portion
// of that region to `output_path` so it can be inspected offline.
//
// Non-printable bytes are replaced with '.' so the file is text-editor friendly.
// Saves up to 8 MB centred on the MiscItems key (4 MB before, 4 MB after).

#[cfg(target_os = "windows")]
pub fn capture_inventory_blob(output_path: &std::path::Path) -> Result<String, String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = find_warframe_pid_pub().ok_or_else(|| "Warframe is not running".to_string())?;

    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 { return Err("Could not open Warframe process".to_string()); }

    const MISC_KEY: &[u8]      = b"\"MiscItems\":[";
    const MIN_BLOB_BYTES: usize = 200_000;    // skip tiny chunks — real inventory is MB-scale
    const MAX_REGION_READ: usize = 128 * 1024 * 1024;
    const HALF_SAVE: usize      = 4 * 1024 * 1024;   // 4 MB either side of MiscItems

    let mut addr: usize = 0;
    let mut saved: Option<(usize, String)> = None; // (region size, message)

    'outer: loop {
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }

        let region_addr = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let next_addr   = region_addr.saturating_add(region_size);

        if mbi.State == MEM_COMMIT
            && mbi.Protect & PAGE_GUARD    == 0
            && mbi.Protect & PAGE_NOACCESS == 0
            && region_size >= MIN_BLOB_BYTES
            && region_size <= MAX_REGION_READ
        {
            let mut data = vec![0u8; region_size];
            let mut n = 0usize;
            if unsafe { ReadProcessMemory(process, region_addr as *const c_void, data.as_mut_ptr() as *mut c_void, region_size, &mut n) } != 0 && n >= MIN_BLOB_BYTES {
                let data = &data[..n];
                if let Some(misc_pos) = data.windows(MISC_KEY.len()).position(|w| w == MISC_KEY) {
                    let start = misc_pos.saturating_sub(HALF_SAVE);
                    let end   = (misc_pos + HALF_SAVE).min(data.len());
                    let text: Vec<u8> = data[start..end].iter()
                        .map(|&b| if b >= 0x20 && b <= 0x7e || b == b'\n' || b == b'\t' { b } else { b'.' })
                        .collect();
                    if let Err(e) = std::fs::write(output_path, &text) {
                        unsafe { CloseHandle(process); }
                        return Err(format!("Write failed: {e}"));
                    }
                    saved = Some((text.len(), format!(
                        "Saved {}KB blob (region 0x{:x}, size {}KB, MiscItems at +{}KB) to {}",
                        text.len() / 1024, region_addr, n / 1024, misc_pos / 1024,
                        output_path.display()
                    )));
                    break 'outer;
                }
            }
        }

        if next_addr <= addr { break; }
        addr = next_addr;
    }

    unsafe { CloseHandle(process); }

    saved.map(|(_, msg)| msg)
         .ok_or_else(|| "No inventory blob found — make sure Warframe is running and inventory is loaded (open Arsenal or Inventory screen)".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn capture_inventory_blob(_output_path: &std::path::Path) -> Result<String, String> {
    Err("Only supported on Windows".into())
}

/// Scan all Warframe process memory and save every relevant blob found into `blob_dir`.
/// "Relevant" = region ≥ 100 KB that contains at least one of: MiscItems, Suits,
// ─── Full-account blob parser ─────────────────────────────────────────────────

/// Find the end of the FULL_ACCOUNT blob by locating `"DeathSquadable":` and
/// the `}` that immediately follows its boolean value (true or false).
fn find_blob_end(raw: &[u8]) -> Option<usize> {
    const KEY: &[u8] = b"\"DeathSquadable\":";
    let key_pos = raw.windows(KEY.len()).position(|w| w == KEY)?;
    let after   = key_pos + KEY.len();
    // Skip the boolean value and find the closing brace
    let brace = raw[after..].iter().position(|&b| b == b'}')?;
    Some(after + brace + 1)
}

const START_MARKER: &[u8] = b"\"SubscribedToEmails\"";
const ALT_STARTS: &[&[u8]] = &[
    b"\"MiscItems\":[{\"ItemType\":\"/Lotus/",
    b"\"Suits\":[{\"ItemType\":\"/Lotus/",
    b"\"RegularCredits\":",
];

/// Walk backward from `marker_off` counting braces; return the offset of the
/// outermost `{` that encloses the marker.  Returns `None` when the buffer is
/// inconsistent (freed/partial blob where the opening brace was overwritten).
fn enclosing_object_start(buf: &[u8], marker_off: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for i in (0..marker_off).rev() {
        match buf[i] {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth < 0 { return Some(i); }
            }
            _ => {}
        }
    }
    None
}

/// Locate seed offsets for the FULL_ACCOUNT blob within `buf`.
/// Returns `(marker_off, json_open)`:
///   - `marker_off`: byte position of the first known start marker
///   - `json_open`:  byte position of the outermost `{` enclosing the marker
fn blob_seed_offsets(buf: &[u8]) -> (usize, usize) {
    let marker_off = buf.windows(START_MARKER.len())
        .position(|w| w == START_MARKER)
        .or_else(|| ALT_STARTS.iter().find_map(|a|
            buf.windows(a.len()).position(|w| w == *a)))
        .unwrap_or(buf.len().saturating_sub(1));
    let json_open = enclosing_object_start(buf, marker_off).unwrap_or(marker_off);
    (marker_off, json_open)
}

/// Parse a FULL_ACCOUNT blob from raw memory bytes into structured inventory data.
///
/// Compute the deterministic riven mod name from its buff stats.
/// Mirrors the RIVEN_NAME_PARTS table in MarketHelper.tsx.
/// 1 buff  → coreSuffix       (buff's prefix word + buff's suffix word)
/// 2 buffs → coreSuffix       (higher's prefix + lower's suffix, no dash)
/// 3 buffs → prefix-coreSuffix (highest - second + lowest, with dash)
pub fn compute_riven_mod_name(buffs: &[BlobRivenStat]) -> String {
    fn parts(tag: &str) -> Option<(&'static str, &'static str)> {
        match tag {
            "WeaponMeleeComboBonusOnHitMod" | "WeaponMeleeComboPointsOnHitMod" => Some(("Laci",  "Nus"  )),
            "WeaponAmmoMaxMod"                                                  => Some(("Ampi",  "Bin"  )),
            "WeaponMeleeFactionDamageCorpus"   | "WeaponFactionDamageCorpus"   => Some(("Manti", "Tron" )),
            "WeaponMeleeFactionDamageGrineer"  | "WeaponFactionDamageGrineer"  => Some(("Argi",  "Con"  )),
            "WeaponMeleeFactionDamageInfested" | "WeaponFactionDamageInfested" => Some(("Pura",  "Ada"  )),
            "WeaponFreezeDamageMod"            => Some(("Geli",  "Do"   )),
            "ComboDurationMod"                 => Some(("Tempi", "Nem"  )),
            "WeaponCritChanceMod"              => Some(("Crita", "Cron" )),
            "SlideAttackCritChanceMod"         => Some(("Pleci", "Nent" )),
            "WeaponCritDamageMod"              => Some(("Acri",  "Tis"  )),
            "WeaponDamageAmountMod" | "WeaponMeleeDamageMod" => Some(("Visi", "Ata")),
            "WeaponElectricityDamageMod"       => Some(("Vexi",  "Tio"  )),
            "WeaponFireDamageMod"              => Some(("Igni",  "Pha"  )),
            "WeaponMeleeFinisherDamageMod"     => Some(("Exi",   "Cta"  )),
            "WeaponFireRateMod"                => Some(("Croni", "Dra"  )),
            "WeaponProjectileSpeedMod"         => Some(("Conci", "Nak"  )),
            "WeaponMeleeComboInitialBonusMod"  => Some(("Para",  "Um"   )),
            "WeaponImpactDamageMod"            => Some(("Magna", "Ton"  )),
            "WeaponClipMaxMod"                 => Some(("Arma",  "Tin"  )),
            "WeaponMeleeComboEfficiencyMod"    => Some(("Forti", "Us"   )),
            "WeaponFireIterationsMod"          => Some(("Sati",  "Can"  )),
            "WeaponToxinDamageMod"             => Some(("Toxi",  "Tox"  )),
            "WeaponPunctureDepthMod"           => Some(("Lexi",  "Nok"  )),
            "WeaponArmorPiercingDamageMod"     => Some(("Insi",  "Cak"  )),
            "WeaponReloadSpeedMod"             => Some(("Feva",  "Tak"  )),
            "WeaponMeleeRangeIncMod"           => Some(("Locti", "Tor"  )),
            "WeaponSlashDamageMod"             => Some(("Sci",   "Sus"  )),
            "WeaponStunChanceMod"              => Some(("Hexa",  "Dex"  )),
            "WeaponProcTimeMod"                => Some(("Deci",  "Des"  )),
            "WeaponRecoilReductionMod"         => Some(("Zeti",  "Mag"  )),
            "WeaponZoomFovMod"                 => Some(("Hera",  "Lis"  )),
            _ => None,
        }
    }
    if buffs.is_empty() { return String::new(); }
    let mut sorted: Vec<&BlobRivenStat> = buffs.iter().collect();
    sorted.sort_by(|a, b| b.value.cmp(&a.value));
    let Some((hi_p, _))  = parts(&sorted[0].tag)                   else { return String::new(); };
    let Some((_, lo_s))  = parts(&sorted[sorted.len() - 1].tag)    else { return String::new(); };
    if sorted.len() >= 3 {
        if let Some((mid_p, _)) = parts(&sorted[1].tag) {
            return format!("{}-{}{}", hi_p.to_lowercase(), mid_p.to_lowercase(), lo_s.to_lowercase());
        }
    }
    format!("{}{}", hi_p.to_lowercase(), lo_s.to_lowercase())
}

/// `raw` must span from the JSON opening `{` (or from `"SubscribedToEmails"`) through
/// `"DeathSquadable":`. Returns `None` if neither start can be located or JSON is malformed.
pub fn parse_full_account_blob(raw: &[u8]) -> Option<BlobInventory> {
    let end_pos = find_blob_end(raw)?;

    // Real FULL_ACCOUNT blobs are several hundred KB — anything smaller is a
    // small false-positive fragment that matched the end marker by coincidence.
    const MIN_PARSE_BYTES: usize = 50_000;
    if end_pos < MIN_PARSE_BYTES {
        eprintln!("[blob-parse] too small ({} B < {} B) — skipping", end_pos, MIN_PARSE_BYTES);
        return None;
    }

    // capture_all_blobs seeds from the JSON opening { so raw already forms a complete
    // JSON object — use it directly. Fall back to the old SubscribedToEmails approach
    // for any caller that still passes data starting inside the object.
    let json_bytes: Vec<u8> = if raw.first() == Some(&b'{') {
        raw[..end_pos].to_vec()
    } else {
        const START: &[u8] = b"\"SubscribedToEmails\"";
        let start_pos = raw.windows(START.len()).position(|w| w == START)?;
        let mut v = Vec::with_capacity(end_pos - start_pos + 1);
        v.push(b'{');
        v.extend_from_slice(&raw[start_pos..end_pos]);
        v
    };

    let json: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| {
            let head: String = json_bytes[..json_bytes.len().min(48)]
                .iter().map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '.' }).collect();
            eprintln!("[blob-parse] JSON error: {} | head: {:?}", e, head);
        })
        .ok()?;

    // Scalars
    let credits       = json["RegularCredits"].as_i64().unwrap_or(0);
    let endo          = json["FusionPoints"].as_i64().unwrap_or(0);
    let platinum      = json["PremiumCredits"].as_i64().unwrap_or(0);
    let free_platinum = json["PremiumCreditsFree"].as_i64().unwrap_or(0);
    let mastery_level = json["PlayerLevel"].as_u64().unwrap_or(0) as u32;

    // Unique item sections — each array entry = one owned copy
    const UNIQUE_SECS: &[&str] = &[
        "Suits", "LongGuns", "Pistols", "Melee",
        "SpaceSuits", "SpaceMelee", "SpaceGuns",
        "Sentinels", "SentinelWeapons", "KubrowPets",
        "OperatorAmps", "MechSuits",
    ];
    let mut unique_items = Vec::new();
    for &sec in UNIQUE_SECS {
        if let Some(arr) = json[sec].as_array() {
            for e in arr {
                let Some(it) = e["ItemType"].as_str() else { continue };
                if !it.starts_with("/Lotus/") { continue; }
                let archon_shards = e["ArchonCrystalUpgrades"].as_array()
                    .map(|a| a.iter().filter_map(|s| {
                        Some(ArchonShard {
                            color:        s["Color"].as_str()?.to_string(),
                            upgrade_type: s["UpgradeType"].as_str().unwrap_or("").to_string(),
                        })
                    }).collect())
                    .unwrap_or_default();
                unique_items.push(BlobUniqueEntry {
                    item_type:     it.to_string(),
                    section:       sec.to_string(),
                    polarized:     e["Polarized"].as_u64().unwrap_or(0) as u32,
                    pet_name:      e["Details"]["Name"].as_str().map(String::from),
                    focus_lens:    e["FocusLens"].as_str().map(String::from),
                    archon_shards,
                });
            }
        }
    }

    // Stackable item sections
    const STACK_SECS: &[(&str, bool)] = &[
        ("MiscItems",          false),
        ("Recipes",            false),
        ("FusionTreasures",    true),   // has Sockets
        ("CrewShipRawSalvage", false),
        ("ShipDecorations",    false),
    ];
    let mut stackable_items = Vec::new();
    for &(sec, has_sockets) in STACK_SECS {
        if let Some(arr) = json[sec].as_array() {
            for e in arr {
                let Some(it) = e["ItemType"].as_str() else { continue };
                if !it.starts_with("/Lotus/") { continue; }
                let count = e["ItemCount"].as_i64().unwrap_or(0);
                if count <= 0 { continue; }
                stackable_items.push(BlobStackableEntry {
                    item_type:  it.to_string(),
                    item_count: count,
                    sockets:    if has_sockets { e["Sockets"].as_i64() } else { None },
                });
            }
        }
    }

    // Rivens + Mods: RawUpgrades (unranked, ItemCount) + Upgrades (ranked, one entry = one copy).
    // Riven paths contain "RandomMod" — extract them separately and skip from mods map.
    let mut rivens: Vec<BlobRivenEntry> = Vec::new();
    let mut mods: HashMap<String, ModCount> = HashMap::new();
    if let Some(arr) = json["RawUpgrades"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            let count = e["ItemCount"].as_i64().unwrap_or(0);
            if count <= 0 { continue; }
            if it.contains("RandomMod") {
                // Unrevealed riven: stacked in RawUpgrades, only type visible.
                rivens.push(BlobRivenEntry {
                    item_id:  String::new(),
                    item_type: it.to_string(),
                    riven_state: RivenState::Unrevealed,
                    compat: None, challenge_type: None, challenge_complication: None,
                    lvl_req: None, polarity: None,
                    buffs: vec![], curses: vec![],
                    mod_rank: 0, count: count as u32, rerolls: 0,
                    mod_name: String::new(),
                });
                continue;
            }
            let mc = mods.entry(it.to_string()).or_default();
            *mc.by_rank.entry(0).or_insert(0) += count;
            mc.total += count;
        }
    }
    if let Some(arr) = json["Upgrades"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            if it.contains("RandomMod") {
                let fp_str = e["UpgradeFingerprint"].as_str().unwrap_or("{}");
                if let Ok(fp) = serde_json::from_str::<serde_json::Value>(fp_str) {
                    let item_id = e["ItemId"]["$oid"].as_str().unwrap_or("").to_string();
                    if let Some(compat) = fp["compat"].as_str() {
                        // Unlocked riven: weapon assigned + stats visible.
                        let buffs: Vec<BlobRivenStat> = fp["buffs"].as_array()
                            .map(|a| a.iter().filter_map(|s| Some(BlobRivenStat {
                                tag:   s["Tag"].as_str()?.to_string(),
                                value: s["Value"].as_i64().unwrap_or(0),
                            })).collect())
                            .unwrap_or_default();
                        let curses: Vec<BlobRivenStat> = fp["curses"].as_array()
                            .map(|a| a.iter().filter_map(|s| Some(BlobRivenStat {
                                tag:   s["Tag"].as_str()?.to_string(),
                                value: s["Value"].as_i64().unwrap_or(0),
                            })).collect())
                            .unwrap_or_default();
                        let mod_name = compute_riven_mod_name(&buffs);
                        rivens.push(BlobRivenEntry {
                            item_id, item_type: it.to_string(),
                            riven_state: RivenState::Unlocked,
                            compat: Some(compat.to_string()),
                            challenge_type: None, challenge_complication: None,
                            lvl_req:  fp["lvlReq"].as_u64().map(|v| v as u32),
                            polarity: fp["pol"].as_str().map(String::from),
                            mod_rank: fp["lvl"].as_u64().map(|v| v as u8).unwrap_or(0),
                            count: 1,
                            rerolls: fp["rerolls"].as_u64().unwrap_or(0) as u32,
                            mod_name,
                            buffs,
                            curses,
                        });
                        continue;
                    } else if fp["challenge"].is_object() {
                        // Revealed riven: challenge assigned but not yet completed.
                        let challenge_type = fp["challenge"]["Type"].as_str().map(String::from);
                        let challenge_complication = fp["challenge"]["Complication"].as_str().map(String::from);
                        rivens.push(BlobRivenEntry {
                            item_id, item_type: it.to_string(),
                            riven_state: RivenState::Revealed,
                            compat: None, challenge_type, challenge_complication,
                            lvl_req: None, polarity: None,
                            buffs: vec![], curses: vec![],
                            mod_rank: 0, count: 1, rerolls: 0,
                            mod_name: String::new(),
                        });
                        continue;
                    }
                }
            }
            let rank = blob_extract_mod_rank(e["UpgradeFingerprint"].as_str());
            let mc = mods.entry(it.to_string()).or_default();
            *mc.by_rank.entry(rank).or_insert(0) += 1;
            mc.total += 1;
        }
    }

    // FlavourItems (glyphs, palettes, emotes, titles, ship skins): each entry = one copy.
    let mut flavour_items: HashMap<String, i64> = HashMap::new();
    if let Some(arr) = json["FlavourItems"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            *flavour_items.entry(it.to_string()).or_insert(0) += 1;
        }
    }

    // WeaponSkins (sigils, cosmetic skins): each array entry = one owned copy,
    // count occurrences of the same ItemType.
    let mut weapon_skins: HashMap<String, i64> = HashMap::new();
    if let Some(arr) = json["WeaponSkins"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            *weapon_skins.entry(it.to_string()).or_insert(0) += 1;
        }
    }

    // XPInfo → mastery ranks (covers items no longer owned)
    let mut mastery_data: HashMap<String, u32> = HashMap::new();
    if let Some(arr) = json["XPInfo"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if let Some(xp) = e["XP"].as_i64() {
                let rank = xp_to_rank(xp, it);
                if rank > 0 { mastery_data.insert(it.to_string(), rank); }
            }
        }
    }

    // PendingRecipes (Foundry)
    let pending_recipes: Vec<BlobPendingRecipe> = json["PendingRecipes"].as_array()
        .map(|a| a.iter().filter_map(|e| {
            let it = e["ItemType"].as_str()?.to_string();
            let ms = e["CompletionDate"]["$date"]["$numberLong"]
                .as_str().and_then(|s| s.parse::<i64>().ok())
                .or_else(|| e["CompletionDate"]["$date"]["$numberLong"].as_i64())
                .unwrap_or(0);
            Some(BlobPendingRecipe { item_type: it, completion_ms: ms })
        }).collect())
        .unwrap_or_default();

    // Helminth consumed suits
    let consumed_suits: Vec<String> = json["InfestedFoundry"]["ConsumedSuits"].as_array()
        .map(|a| a.iter().filter_map(|e| e["s"].as_str().map(String::from)).collect())
        .unwrap_or_default();

    Some(BlobInventory {
        credits, endo, platinum, free_platinum, mastery_level,
        unique_items, stackable_items, mods,
        flavour_items, weapon_skins, mastery_data, pending_recipes, consumed_suits,
        rivens,
    })
}

/// Extract the `lvl` field from a mod UpgradeFingerprint JSON string.
/// Returns 0 for unranked or missing fingerprint.
fn blob_extract_mod_rank(fingerprint: Option<&str>) -> u8 {
    fingerprint
        .and_then(|fp| {
            let pos = fp.find("\"lvl\":")?;
            let after = fp[pos + 6..].trim_start();
            let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
            after[..end].parse::<u8>().ok()
        })
        .unwrap_or(0)
}

// ─── Blob capture ─────────────────────────────────────────────────────────────

// Cache: remember the region address where the blob was last successfully found.
// On the next cycle we probe that address first — if the blob is still there we
// finish in milliseconds instead of walking the full address space.
static LAST_BLOB_REGION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Clear the fast-path region cache. Call when Warframe's PID changes so the
/// next scan doesn't probe a stale address from the previous process instance.
pub fn reset_last_blob_region() {
    LAST_BLOB_REGION.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Scans Warframe process memory for the FULL_ACCOUNT inventory blob and sends it
/// through `blob_tx` for the monitor loop to apply.
///
/// Multi-scan strategy: the blob may span many memory regions and multiple copies
/// can exist at different addresses. We track every potential start point as a
/// separate in-flight scan and stitch them all in parallel as the region walk
/// advances. The first scan that produces a valid JSON blob wins; all others are
/// dropped. This is far more robust than the old single-start approach when the
/// blob is large or when the first start hit leads to a truncated region.
///
/// Algorithm:
///   1. Walk every committed readable region.
///   2. If a region has START_MARKER ("SubscribedToEmails") and is NOT a mission
///      delta ("InventoryChanges"), open a new ActiveScan seeded with that region's
///      data from the START_MARKER offset onwards.
///   3. Every readable region is appended to ALL active scans (stitching).
///   4. After each append, check every scan for the end marker. If found, parse it.
///      On success send the inventory to the monitor loop. On failure drop the scan.
///      The walk always continues through all of memory — every blob start is found.
///   5. Drop any scan that grows past MAX_SCAN_BYTES without finding the end.
///
/// When `save=true` also writes the raw text to `blob_dir` for debugging.
/// Returns the number of files written (always 0 when `save=false`).
#[cfg(target_os = "windows")]
pub fn capture_all_blobs(blob_dir: &std::path::Path, ts: &str, blob_tx: std::sync::mpsc::Sender<BlobInventory>, save: bool) -> usize {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = match find_warframe_pid_pub() { Some(p) => p, None => return 0 };
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 { return 0; }

    const MIN_REGION:    usize = 64_000;   // skip regions smaller than 64 KB
    const MAX_READ:      usize = 64  * 1024 * 1024;
    const MAX_SCAN:      usize = 20  * 1024 * 1024;
    const MAX_BLOBS:     usize = 25;

    // Executable pages never contain heap data — safe to skip.
    const PAGE_EXECUTE:      u32 = 0x10;
    const PAGE_EXECUTE_READ: u32 = 0x20;
    const PAGE_EXECUTE_RW:   u32 = 0x40;
    const PAGE_EXECUTE_WC:   u32 = 0x80;
    const EXEC_MASK: u32 = PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_RW | PAGE_EXECUTE_WC;

    const MISSION_DELTA: &[u8] = b"\"InventoryChanges\":";
    const LOTUS_KEY:     &[u8] = b"/Lotus/";
    const ANCHORS: &[&[u8]] = &[
        b"\"SubscribedToEmails\"",
        b"\"MiscItems\":[",
        b"\"Suits\":[",
        b"\"LongGuns\":[",
        b"\"Melee\":[",
        b"\"Pistols\":[",
    ];

    // ── Fast path: try the cached region from last successful scan ─────────────
    // If the blob is still at the same address, we skip the entire memory walk.
    let cached_addr = LAST_BLOB_REGION.load(std::sync::atomic::Ordering::Relaxed) as usize;
    if cached_addr != 0 && !save {
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        let ok = unsafe { VirtualQueryEx(process, cached_addr as *const c_void, &mut mbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } != 0;
        if ok && mbi.State == MEM_COMMIT
            && mbi.Protect & PAGE_GUARD == 0
            && mbi.Protect & PAGE_NOACCESS == 0
        {
            let read_cap = mbi.RegionSize.min(MAX_READ);
            let mut buf = vec![0u8; read_cap];
            let mut n = 0usize;
            let read_ok = unsafe { ReadProcessMemory(process, cached_addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void, read_cap, &mut n) } != 0 && n >= 8;
            if read_ok {
                let chunk = &buf[..n];
                let is_mission = chunk.windows(MISSION_DELTA.len()).any(|w| w == MISSION_DELTA);
                let has_anchor = ANCHORS.iter().any(|a| chunk.windows(a.len()).any(|w| w == *a));
                let has_lotus  = chunk.windows(LOTUS_KEY.len()).any(|w| w == LOTUS_KEY);
                // cached_addr is the exact byte of the blob's outer {, so seed from byte 0.
                // Accept regions that are blob data even when SubscribedToEmails is in a
                // later region (field order varies by account).
                if !is_mission && (has_anchor || has_lotus) && chunk.starts_with(b"{\"") {
                    let mut stitched = chunk.to_vec();
                    let mut walk = cached_addr + n;
                    while stitched.len() < MAX_SCAN && find_blob_end(&stitched).is_none() {
                        let mut nmbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
                        if unsafe { VirtualQueryEx(process, walk as *const c_void, &mut nmbi,
                            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }
                        let nr = nmbi.BaseAddress as usize;
                        let ns = nmbi.RegionSize;
                        walk = nr + ns;
                        if nmbi.State != MEM_COMMIT
                            || nmbi.Protect & PAGE_GUARD != 0
                            || nmbi.Protect & PAGE_NOACCESS != 0
                            || ns == 0 { continue; }
                        let cap = ns.min(MAX_READ);
                        let mut nb = vec![0u8; cap];
                        let mut nn = 0usize;
                        if unsafe { ReadProcessMemory(process, nr as *const c_void,
                            nb.as_mut_ptr() as *mut c_void, cap, &mut nn) } == 0 { continue; }
                        stitched.extend_from_slice(&nb[..nn]);
                    }
                    if let Some(inv) = parse_full_account_blob(&stitched) {
                        eprintln!("[blob] fast-path hit at 0x{:012x}: {} unique, {} stackable",
                            cached_addr, inv.unique_items.len(), inv.stackable_items.len());
                        blob_tx.send(inv).ok();
                        unsafe { CloseHandle(process); }
                        return 0; // fast path never saves to disk
                    }
                }
            }
        }
        // Cache miss — fall through to full walk and update the cache when found
        eprintln!("[blob] fast-path miss at 0x{:012x} — doing full walk", cached_addr);
    }

    struct ActiveScan {
        data: Vec<u8>,
        id: usize,
        /// Base address of the region where this scan was seeded (JSON start).
        /// Used to update LAST_BLOB_REGION correctly for multi-region blobs.
        start_region_addr: usize,
        /// Minimum offset at which the end-marker search should start next append.
        /// Avoids rescanning already-checked data on every region append (O(n²) → O(n)).
        search_from: usize,
    }
    let mut scans: Vec<ActiveScan> = Vec::new();
    let mut next_scan_id = 0usize;

    // Pre-buffer: rolling window of recent regions that have Lotus paths / anchor keys
    // but no SubscribedToEmails yet.  Used to recover the true JSON start when the
    // outer `{` of the FULL_ACCOUNT blob lives in a region that precedes the region
    // containing SubscribedToEmails (field order varies by account).
    struct PreChunk { addr: usize, end_addr: usize, data: Vec<u8> }
    let mut pre_buf: std::collections::VecDeque<PreChunk> = std::collections::VecDeque::new();
    const PRE_BUF_BYTES: usize = 8 * 1024 * 1024; // keep ≤8 MB of prefix history

    let mut addr: usize = 0;
    let mut saved = 0usize;
    let mut regions_skipped = 0usize;
    let mut regions_read    = 0usize;
    let mut starts_found    = 0usize;
    let mut t_vquery = std::time::Duration::ZERO;
    let mut t_read   = std::time::Duration::ZERO;
    let mut t_search = std::time::Duration::ZERO;
    let mut bytes_read: u64 = 0;
    // Once we have at least one successful parse we stop opening new scans.
    // Active scans already in progress are still stitched to completion (or dropped).
    // The loop exits as soon as all active scans are gone.
    let mut found_result = false;

    loop {
        if saved >= MAX_BLOBS { break; }
        // Early exit: we have a result and no active scans left to finish.
        if found_result && scans.is_empty() && !save { break; }

        let t0 = std::time::Instant::now();
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }
        t_vquery += t0.elapsed();

        let region_addr = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let next_addr   = region_addr.saturating_add(region_size);
        if next_addr <= addr { break; }
        addr = next_addr;

        // ── Region filters ──────────────────────────────────────────────────
        // Skip pages that can never hold heap JSON:
        // • must be committed and readable
        // • skip execute-only pages (code sections, JIT stubs)
        // • skip PE image sections — those hold string constants in the exe/DLLs,
        //   not live heap data; they false-trigger the Lotus anchor check and
        //   cost ~40 s scanning 20 MB+ without ever finding the blob end
        // • skip anything smaller than MIN_REGION
        if mbi.State   != MEM_COMMIT
            || mbi.Protect &  PAGE_GUARD    != 0
            || mbi.Protect &  PAGE_NOACCESS != 0
            || mbi.Protect &  EXEC_MASK     != 0
            || mbi.Type    == MEM_IMAGE
            || region_size  < MIN_REGION
        { regions_skipped += 1; continue; }

        let read_cap = region_size.min(MAX_READ);

        let t1 = std::time::Instant::now();
        let mut buf = vec![0u8; read_cap];
        let mut n = 0usize;
        if unsafe { ReadProcessMemory(process, region_addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void, read_cap, &mut n) } == 0 || n < 8 {
            regions_skipped += 1; continue;
        }
        t_read += t1.elapsed();
        bytes_read += n as u64;
        let chunk = &buf[..n];
        regions_read += 1;

        // ── Step 1: append this chunk to every active scan and check for completion ──
        // search_from tracks where we left off so we only scan newly-appended bytes
        // (plus a small overlap for markers that straddle a region boundary).
        const END_MARKER: &[u8] = b"\"DeathSquadable\":";
        scans.retain_mut(|scan| {
            // A previous scan in this same retain_mut pass already succeeded.
            // Drop this one immediately — applying a second blob overwrites correct data
            // with a stale/parallel copy from a different memory region.
            if found_result && !save { return false; }
            // Advance the search cursor before appending so the overlap catches split markers.
            let search_from = scan.search_from;
            scan.search_from = scan.data.len().saturating_sub(END_MARKER.len() - 1);
            scan.data.extend_from_slice(chunk);
            if scan.data.len() > MAX_SCAN {
                eprintln!("[blob] scan#{} exceeded {} MB without end — dropped", scan.id, MAX_SCAN / 1024 / 1024);
                return false; // drop oversized scan
            }
            // Only search the newly-added window, not the full buffer.
            let has_end = scan.data[search_from..]
                .windows(END_MARKER.len())
                .any(|w| w == END_MARKER);
            if has_end && find_blob_end(&scan.data).is_some() {
                match parse_full_account_blob(&scan.data) {
                    Some(inv) => {
                        eprintln!("[blob] scan#{} SUCCESS at 0x{:012x}: {} unique, {} stackable, {} mods",
                            scan.id, region_addr, inv.unique_items.len(), inv.stackable_items.len(), inv.mods.len());
                        // Cache the START region (not this region) so the fast path works next cycle.
                        LAST_BLOB_REGION.store(scan.start_region_addr as u64, std::sync::atomic::Ordering::Relaxed);
                        if save {
                            let name = format!("Actual_inventory_FULL_ACCOUNT_{}_{:02}.txt", ts, saved + 1);
                            let path = blob_dir.join(&name);
                            let text: Vec<u8> = scan.data.iter()
                                .map(|&b| if b >= 0x20 && b <= 0x7e || b == b'\n' || b == b'\t' { b } else { b'.' })
                                .collect();
                            if std::fs::write(&path, &text).is_ok() { saved += 1; }
                        }
                        blob_tx.send(inv).ok();
                        found_result = true;
                    }
                    None => {
                        eprintln!("[blob] scan#{} end marker found but JSON parse failed — dropped", scan.id);
                    }
                }
                false // remove completed (or failed) scan
            } else {
                true // keep waiting for end
            }
        });

        // ── Step 2: check if this chunk opens a new scan ──
        // Don't open new scans once we already have a result — drain the active ones then exit.
        if found_result { continue; }

        let t2 = std::time::Instant::now();
        let has_start     = chunk.windows(START_MARKER.len()).any(|w| w == START_MARKER);
        let has_alt_start = ALT_STARTS.iter().any(|a| chunk.windows(a.len()).any(|w| w == *a));
        let is_mission    = chunk.windows(MISSION_DELTA.len()).any(|w| w == MISSION_DELTA);
        let has_anchor    = ANCHORS.iter().any(|a| chunk.windows(a.len()).any(|w| w == *a));
        let has_lotus     = chunk.windows(LOTUS_KEY.len()).any(|w| w == LOTUS_KEY);
        let qualifies     = (has_start || has_alt_start) && !is_mission && (has_anchor || has_lotus);
        t_search += t2.elapsed();

        // Accumulate regions with Lotus paths (no SubscribedToEmails, no mission delta) into
        // a pre-buffer.  When SubscribedToEmails is found in a later region, we prepend
        // contiguous pre-buffer regions so that the backward {"  search finds the true
        // outermost JSON opening rather than a nested {"$oid":…} inside the blob.
        if !has_start && !is_mission && (has_anchor || has_lotus) {
            while pre_buf.iter().map(|p| p.data.len()).sum::<usize>() + n > PRE_BUF_BYTES
                && !pre_buf.is_empty()
            {
                pre_buf.pop_front();
            }
            pre_buf.push_back(PreChunk { addr: region_addr, end_addr: region_addr + n, data: chunk.to_vec() });
        }

        if qualifies {
            // Prepend any contiguous pre-buffer regions that immediately precede this one.
            // This recovers the full blob when the outer { lives in an earlier region and
            // SubscribedToEmails appears later (field order varies per account/build).
            let mut combined: Vec<u8> = Vec::new();
            let mut blob_start_addr = region_addr;
            {
                let mut expect_end = region_addr;
                let mut chain: Vec<usize> = Vec::new();
                for (i, pc) in pre_buf.iter().enumerate().rev() {
                    // Allow ≤4 KB alignment gap between regions.
                    if pc.end_addr <= expect_end && pc.end_addr + 4096 >= expect_end {
                        chain.push(i);
                        expect_end = pc.addr;
                    } else if pc.end_addr < expect_end.saturating_sub(4096) {
                        break;
                    }
                }
                chain.reverse();
                for &i in &chain {
                    let pc = &pre_buf[i];
                    if combined.is_empty() { blob_start_addr = pc.addr; }
                    combined.extend_from_slice(&pc.data);
                }
            }
            combined.extend_from_slice(chunk);

            let (start_off, json_open) = blob_seed_offsets(&combined);

            // Absolute memory address of the seed start (for LAST_BLOB_REGION cache).
            let seed_addr = blob_start_addr + json_open;

            let id = next_scan_id;
            next_scan_id += 1;
            starts_found += 1;
            let pre_bytes = combined.len() - n;
            eprintln!(
                "[blob] scan#{} started at 0x{:012x}+{} (json_open={} seed=0x{:012x} pre={}B)",
                id, region_addr, start_off, json_open, seed_addr, pre_bytes
            );
            let seed = combined[json_open..].to_vec();

            if find_blob_end(&seed).is_some() {
                match parse_full_account_blob(&seed) {
                    Some(inv) => {
                        eprintln!("[blob] scan#{} immediate SUCCESS at 0x{:012x}: {} unique, {} stackable",
                            id, region_addr, inv.unique_items.len(), inv.stackable_items.len());
                        LAST_BLOB_REGION.store(seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
                        if save {
                            let name = format!("Actual_inventory_FULL_ACCOUNT_{}_{:02}.txt", ts, saved + 1);
                            if std::fs::write(blob_dir.join(&name), &seed).is_ok() { saved += 1; }
                        }
                        blob_tx.send(inv).ok();
                        found_result = true;
                    }
                    None => {
                        eprintln!("[blob] scan#{} immediate end found but parse failed — dropping", id);
                    }
                }
            } else {
                scans.push(ActiveScan { data: seed, id, start_region_addr: seed_addr, search_from: 0 });
            }
        }
    }

    eprintln!(
        "[blob-capture] done: read={} skipped={} starts={} saved={} bytes={}MB | \
         vquery={:.0}ms read={:.0}ms search={:.0}ms",
        regions_read, regions_skipped, starts_found, saved, bytes_read / 1_000_000,
        t_vquery.as_secs_f64() * 1000.0,
        t_read.as_secs_f64()   * 1000.0,
        t_search.as_secs_f64() * 1000.0,
    );
    if starts_found == 0 {
        eprintln!("[blob-capture] WARNING: no start-marker found — FULL_ACCOUNT not in memory \
            (game in mission, on login screen, or Arsenal not open?)");
    }
    unsafe { CloseHandle(process); }
    saved
}

#[cfg(target_os = "linux")]
fn scan_linux_inventory_regions(
    process: &LinuxProcess,
    regions: impl IntoIterator<Item = LinuxRegion>,
    mut on_blob: impl FnMut(usize, &[u8], BlobInventory) -> bool,
) {
    const MIN_REGION: usize = 64_000;
    const MAX_READ: usize = 64 * 1024 * 1024;
    const MAX_SCAN: usize = 20 * 1024 * 1024;
    const START_MARKER: &[u8] = b"\"SubscribedToEmails\"";
    const END_MARKER: &[u8] = b"\"DeathSquadable\":";
    const MISSION_DELTA: &[u8] = b"\"InventoryChanges\":";
    const PREFIX_MARKERS: &[&[u8]] = &[
        b"/Lotus/",
        b"\"MiscItems\":[",
        b"\"Suits\":[",
        b"\"LongGuns\":[",
        b"\"Melee\":[",
        b"\"Pistols\":[",
    ];
    const PREFIX_BYTES: usize = 8 * 1024 * 1024;

    // Warframe's address space is several gigabytes, and the naive
    // `windows().any()` form of these searches walks every byte position once
    // per marker — eight separate passes per region, which dominated the scan
    // at roughly 60 MiB/s. One Aho-Corasick automaton answers all three
    // questions in a single SIMD-accelerated pass instead.
    let markers = AhoCorasick::new(
        std::iter::once(START_MARKER)
            .chain(std::iter::once(MISSION_DELTA))
            .chain(PREFIX_MARKERS.iter().copied()),
    )
    .expect("marker set is a valid literal alternation");
    const START_PATTERN: usize = 0;
    const MISSION_PATTERN: usize = 1;
    let end_marker = AhoCorasick::new([END_MARKER]).expect("end marker is a valid literal");

    struct ActiveScan {
        data: Vec<u8>,
        start_address: usize,
        search_from: usize,
    }

    struct PrefixChunk {
        start: usize,
        end: usize,
        data: Vec<u8>,
    }

    let mut scans: Vec<ActiveScan> = Vec::new();
    let mut prefix = std::collections::VecDeque::<PrefixChunk>::new();
    for region in regions {
        if region.executable || region.len < MIN_REGION {
            continue;
        }

        let mut buffer = vec![0; region.len.min(MAX_READ)];
        let read = match process.read(region.start, &mut buffer) {
            Ok(read) if read >= 8 => read,
            Ok(_) | Err(_) => continue,
        };
        let chunk = &buffer[..read];

        let mut index = 0;
        while index < scans.len() {
            let search_from = scans[index].search_from;
            scans[index].search_from = scans[index].data.len().saturating_sub(END_MARKER.len() - 1);
            let remaining = MAX_SCAN.saturating_sub(scans[index].data.len());
            let exceeds_limit = chunk.len() > remaining;
            scans[index]
                .data
                .extend_from_slice(&chunk[..chunk.len().min(remaining)]);

            let complete = end_marker.is_match(&scans[index].data[search_from..])
                && find_blob_end(&scans[index].data).is_some();
            if !complete {
                if exceeds_limit {
                    scans.swap_remove(index);
                    continue;
                }
                index += 1;
                continue;
            }

            let scan = scans.swap_remove(index);
            if let Some(inventory) = parse_full_account_blob(&scan.data) {
                if !on_blob(scan.start_address, &scan.data, inventory) {
                    return;
                }
            }
        }

        let mut is_mission = false;
        let mut start_offset = None;
        let mut has_prefix = false;
        for found in markers.find_iter(chunk) {
            match found.pattern().as_usize() {
                MISSION_PATTERN => is_mission = true,
                START_PATTERN => start_offset = start_offset.or(Some(found.start())),
                _ => has_prefix = true,
            }
        }
        if start_offset.is_none() && !is_mission && has_prefix {
            while prefix.iter().map(|item| item.data.len()).sum::<usize>() + read > PREFIX_BYTES
                && !prefix.is_empty()
            {
                prefix.pop_front();
            }
            prefix.push_back(PrefixChunk {
                start: region.start,
                end: region.start + read,
                data: chunk.to_vec(),
            });
        }
        if is_mission {
            continue;
        }
        let Some(_) = start_offset else {
            continue;
        };

        let mut combined = Vec::new();
        let mut combined_start = region.start;
        let mut expected_end = region.start;
        let mut chain = Vec::new();
        for (index, item) in prefix.iter().enumerate().rev() {
            if item.end <= expected_end && item.end + 4096 >= expected_end {
                chain.push(index);
                expected_end = item.start;
            } else if item.end < expected_end.saturating_sub(4096) {
                break;
            }
        }
        chain.reverse();
        for index in chain {
            let item = &prefix[index];
            if combined.is_empty() {
                combined_start = item.start;
            }
            combined.extend_from_slice(&item.data);
        }
        combined.extend_from_slice(chunk);

        let (_, json_open) = blob_seed_offsets(&combined);
        let start_address = combined_start + json_open;
        let seed = combined[json_open..].to_vec();

        if find_blob_end(&seed).is_some() {
            if let Some(inventory) = parse_full_account_blob(&seed) {
                if !on_blob(start_address, &seed, inventory) {
                    return;
                }
            }
        } else {
            scans.push(ActiveScan {
                data: seed,
                start_address,
                search_from: 0,
            });
        }
    }
}

#[cfg(target_os = "linux")]
pub fn capture_all_blobs(
    blob_dir: &std::path::Path,
    ts: &str,
    blob_tx: std::sync::mpsc::Sender<BlobInventory>,
    save: bool,
) -> usize {
    const MAX_BLOBS: usize = 25;

    let Some(pid) = find_warframe_pid_pub() else {
        eprintln!("[blob-capture] Warframe is not running");
        return 0;
    };
    let process = match LinuxProcess::open(pid) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("[blob-capture] {error}");
            return 0;
        }
    };
    let regions = match linux_process_regions(pid) {
        Ok(regions) => regions,
        Err(error) => {
            eprintln!("[blob-capture] {error}");
            return 0;
        }
    };

    let mut found = 0;
    let mut saved = 0;
    scan_linux_inventory_regions(&process, regions, |address, raw, inventory| {
        found += 1;
        eprintln!(
            "[blob] Linux scan SUCCESS at 0x{address:012x}: {} unique, {} stackable, {} mods",
            inventory.unique_items.len(),
            inventory.stackable_items.len(),
            inventory.mods.len()
        );
        if save {
            let path = blob_dir.join(format!(
                "Actual_inventory_FULL_ACCOUNT_{}_{:02}.txt",
                ts,
                saved + 1
            ));
            match std::fs::write(&path, raw) {
                Ok(()) => saved += 1,
                Err(error) => {
                    eprintln!("[blob-capture] Failed to write {}: {error}", path.display())
                }
            }
        }
        blob_tx.send(inventory).ok();
        save && found < MAX_BLOBS
    });

    if found == 0 {
        eprintln!(
            "[blob-capture] WARNING: no FULL_ACCOUNT blob found (open Arsenal or Inventory and try again)"
        );
    }
    saved
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn capture_all_blobs(_blob_dir: &std::path::Path, _ts: &str, _blob_tx: std::sync::mpsc::Sender<BlobInventory>, _save: bool) -> usize { 0 }

// ─── Continuous raw memory string dump ───────────────────────────────────────
//
// Scans every committed readable region in the Warframe process and extracts
// every run of 12+ consecutive printable ASCII bytes.  Each string is written
// to `out_file` as: `0xADDR  <string>\n`.  No needle filtering — everything.
//
// Designed to be called repeatedly from a loop: one call = one full pass.
// Returns the number of strings written this pass, or an error string.
//
// Large regions (>64 MB) are read in 64 MB chunks so the heap stays bounded.
// The caller is responsible for not holding the file lock across sleeps.

#[cfg(target_os = "windows")]
pub fn raw_scan_pass(out: &mut impl std::io::Write) -> Result<usize, String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    const MIN_LEN:  usize = 8;
    const CHUNK:    usize = 64 * 1024 * 1024;
    const TIMEOUT:  u64   = 600; // 10 minutes — full coverage over full scan

    let pid = find_warframe_pid().ok_or("Warframe not running")?;
    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return Err("OpenProcess failed".into()); }

    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT);
    let mut count = 0usize;

    while std::time::Instant::now() < deadline {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        if mbi.State != MEM_COMMIT { continue; }
        let p = mbi.Protect;
        if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
        // Only skip pure-execute (no read bit) — PAGE_EXECUTE_READ (0x20) is kept
        // because game DLL const-string sections use that protection.
        if p == 0x10 { continue; }

        let chunks = (mbi.RegionSize + CHUNK - 1) / CHUNK;
        for ci in 0..chunks {
            if std::time::Instant::now() >= deadline { break; }
            let off        = ci * CHUNK;
            let read_size  = CHUNK.min(mbi.RegionSize - off);
            let chunk_base = mbi.BaseAddress as usize + off;

            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = unsafe {
                ReadProcessMemory(process, chunk_base as *const c_void,
                    buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read)
            };
            if ok == 0 || bytes_read < MIN_LEN { continue; }

            // Extract printable ASCII runs of MIN_LEN+
            let data = &buf[..bytes_read];
            let mut run_start: Option<usize> = None;
            for (i, &b) in data.iter().enumerate() {
                let printable = b >= 0x20 && b < 0x7f;
                if printable {
                    if run_start.is_none() { run_start = Some(i); }
                } else {
                    if let Some(s) = run_start.take() {
                        let len = i - s;
                        if len >= MIN_LEN {
                            let s_str = std::str::from_utf8(&data[s..i]).unwrap_or("?");
                            let _ = writeln!(out, "0x{:012x}  {}", chunk_base + s, s_str);
                            count += 1;
                        }
                    }
                }
            }
            // flush any run that reaches end of chunk
            if let Some(s) = run_start {
                let len = bytes_read - s;
                if len >= MIN_LEN {
                    let s_str = std::str::from_utf8(&data[s..bytes_read]).unwrap_or("?");
                    let _ = writeln!(out, "0x{:012x}  {}", chunk_base + s, s_str);
                    count += 1;
                }
            }
        }
    }

    unsafe { CloseHandle(process); }
    Ok(count)
}

#[cfg(not(target_os = "windows"))]
pub fn raw_scan_pass(_out: &mut impl std::io::Write) -> Result<usize, String> {
    Err("Only supported on Windows".into())
}

// ─── Riven validity flag scanner ──────────────────────────────────────────────
//
// GEP (gep_warframeext.dll) uses Pattern D-2 to locate a single byte in
// Warframe's .text section that acts as an open/closed flag for the riven
// reroll UI. The byte is non-zero while the screen is shown, zero when closed.
//
// Pattern D-2 (13 bytes):
//   80 3d ?? ?? ?? ?? 00  48 8b ?? ??  0f 85
//   CMP byte ptr [RIP+disp32], 0   MOV ...   JNZ ...
//
// Resolving the flag VA:
//   The CMP instruction is 7 bytes. RIP at execution = match_va + 7.
//   flag_va = (match_va + 7) + i32::from_le_bytes(bytes[2..6])

#[cfg(target_os = "windows")]
fn find_pattern_d2(data: &[u8], base_va: usize) -> Option<usize> {
    let len = data.len();
    if len < 13 { return None; }
    for i in 0..len - 13 {
        if data[i]    != 0x80 || data[i+1]  != 0x3d { continue; }
        if data[i+6]  != 0x00 { continue; }
        if data[i+7]  != 0x48 || data[i+8]  != 0x8b { continue; }
        if data[i+11] != 0x0f || data[i+12] != 0x85 { continue; }
        let disp = i32::from_le_bytes([data[i+2], data[i+3], data[i+4], data[i+5]]);
        let flag_va = (base_va + i + 7) as i64 + disp as i64;
        if flag_va > 0x10000 && flag_va < 0x7fff_ffff_ffff {
            return Some(flag_va as usize);
        }
    }
    None
}

/// Scan Warframe's executable image sections for the riven screen validity flag VA.
/// Returns the virtual address of the single byte: non-zero = screen open, 0 = closed.
/// Scans once; caller should cache the result and re-scan only on PID change.
#[cfg(target_os = "windows")]
pub fn find_riven_validity_va(pid: u32) -> Option<usize> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return None; }

    let mut result: Option<usize> = None;
    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let start_time = std::time::Instant::now();

    while start_time.elapsed().as_secs() < 60 && result.is_none() {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        // Only scan committed, executable, memory-mapped PE image regions (MEM_IMAGE = 0x1000000).
        // 0x20 = PAGE_EXECUTE_READ (normal .text), 0x40 = PAGE_EXECUTE_READWRITE (patched pages).
        let is_exec_image = mbi.State == MEM_COMMIT
            && matches!(mbi.Protect, 0x20 | 0x40)
            && mbi.Type == 0x1000000
            && mbi.RegionSize >= 13
            && mbi.RegionSize <= 64 * 1024 * 1024;

        if !is_exec_image { continue; }

        let mut buf = vec![0u8; mbi.RegionSize];
        let mut bytes_read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                process, mbi.BaseAddress as *const c_void,
                buf.as_mut_ptr() as *mut c_void, mbi.RegionSize, &mut bytes_read,
            )
        };
        if ok == 0 || bytes_read < 13 { continue; }

        result = find_pattern_d2(&buf[..bytes_read], mbi.BaseAddress as usize);
    }

    unsafe { CloseHandle(process); }
    result
}

#[cfg(not(target_os = "windows"))]
pub fn find_riven_validity_va(_pid: u32) -> Option<usize> { None }

#[cfg(target_os = "linux")]
fn find_warframe_pid() -> Option<u32> {
    std::fs::read_dir("/proc")
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let pid = entry.file_name().to_str()?.parse().ok()?;
            let command = std::fs::read(entry.path().join("cmdline")).ok()?;
            is_warframe_command(&String::from_utf8_lossy(&command)).then_some(pid)
        })
}

#[cfg(target_os = "windows")]
fn find_warframe_pid() -> Option<u32> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32First, Process32Next,
            PROCESSENTRY32, TH32CS_SNAPPROCESS,
        },
    };
    // CreateToolhelp32Snapshot gives process names without needing OpenProcess,
    // so EAC blocking read access on the game process doesn't prevent detection.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE { return None; }

        let mut entry: PROCESSENTRY32 = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

        let mut found = None;
        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                let name_len = entry.szExeFile.iter().position(|&b| b == 0).unwrap_or(260);
                let name = String::from_utf8_lossy(&entry.szExeFile[..name_len]).to_lowercase();
                if name.starts_with("warframe") && !name.contains("launcher") && !name.contains("companion") {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32Next(snapshot, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(test)]
mod seed_tests {
    use super::{enclosing_object_start, blob_seed_offsets};

    #[test]
    fn enclosing_finds_outer_brace() {
        let buf = b"{\"SubscribedToEmails\":1}";
        let off = buf.windows(b"SubscribedToEmails".len())
            .position(|w| w == b"SubscribedToEmails").unwrap();
        assert_eq!(enclosing_object_start(buf, off), Some(0));
    }

    #[test]
    fn enclosing_skips_nested_braces() {
        let buf = b"{\"nested\":{\"a\":1},\"SubscribedToEmails\":1}";
        let off = buf.windows(b"SubscribedToEmails".len())
            .position(|w| w == b"SubscribedToEmails").unwrap();
        assert_eq!(enclosing_object_start(buf, off), Some(0));
    }

    #[test]
    fn enclosing_returns_none_when_no_open_brace() {
        // Stale/freed blob — the outer { was overwritten
        let buf = b"x\"SubscribedToEmails\":1}";
        assert_eq!(enclosing_object_start(buf, 0), None);
    }

    #[test]
    fn seed_offsets_with_start_marker() {
        let buf = b"{\"SubscribedToEmails\":1,\"RegularCredits\":100}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, 0);
        assert!(marker_off > 0);
    }

    #[test]
    fn seed_offsets_with_alt_start_regular_credits() {
        // No SubscribedToEmails — falls through to RegularCredits alt-start
        let buf = b"{\"RegularCredits\":999}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, 0);
        assert_eq!(marker_off, 1);
    }

    #[test]
    fn seed_offsets_stale_blob_falls_back_to_marker() {
        // The outer { was overwritten — json_open falls back to marker_off
        let buf = b"x\"SubscribedToEmails\":1}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, marker_off);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    #[test]
    fn linux_reader_reads_its_own_mapping() {
        let marker = b"frameforge-linux-reader";
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");
        let mut actual = vec![0; marker.len()];
        let read = process
            .read(marker.as_ptr() as usize, &mut actual)
            .expect("marker address is mapped");
        assert_eq!(read, marker.len());
        assert_eq!(actual, marker);
    }

    #[test]
    fn linux_credential_scan_skips_executable_and_reads_data_regions() {
        let login = br#"{"id":"594144e63ade7f2f2091c48e","Nonce":123456789,"steamId=76561198000000000"}"#;
        let code = login.to_vec();
        let data = login.to_vec();
        let regions = vec![
            LinuxRegion {
                start: code.as_ptr() as usize,
                len: code.len(),
                executable: true,
            },
            LinuxRegion {
                start: data.as_ptr() as usize,
                len: data.len(),
                executable: false,
            },
        ];
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");

        let found = scan_linux_credential_regions(&process, regions);

        assert_eq!(
            found,
            Some((
                "594144e63ade7f2f2091c48e".to_string(),
                "123456789".to_string(),
                "76561198000000000".to_string(),
            ))
        );
    }

    #[test]
    fn linux_inventory_scan_stitches_and_parses_regions() {
        let first_json = br#"{"SubscribedToEmails":true,"RegularCredits":42,"MiscItems":[],"#;
        let second_json = br#""DeathSquadable":false}"#;
        let mut first = first_json.to_vec();
        first.resize(64_000, b' ');
        let mut second = second_json.to_vec();
        second.resize(64_000, 0);
        let regions = vec![
            LinuxRegion {
                start: first.as_ptr() as usize,
                len: first.len(),
                executable: false,
            },
            LinuxRegion {
                start: second.as_ptr() as usize,
                len: second.len(),
                executable: false,
            },
        ];
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");
        let mut inventory = None;

        scan_linux_inventory_regions(&process, regions, |_, _, parsed| {
            inventory = Some(parsed);
            false
        });

        assert_eq!(inventory.expect("inventory blob is found").credits, 42);
    }

    /// The blob is rarely the first thing in the stitched buffer. When a mapping
    /// immediately ahead of it carries a Lotus path it joins the prefix chain,
    /// and seeding at the earliest `{"` in the chain — rather than at the brace
    /// enclosing the blob — produced a document that died on its first value
    /// ("expected value at line 1 column 9") and lost the inventory entirely.
    #[test]
    fn linux_inventory_scan_seeds_at_the_blob_not_at_earlier_json() {
        // Qualifies for the prefix buffer: Lotus path, no start marker, no
        // mission delta — and opens a JSON object of its own.
        let mut prefix = br#"{"Mods":garbage/Lotus/Weapons/Tenno/Rifle "#.to_vec();
        prefix.resize(64_000, b' ');
        let mut blob =
            br#"{"SubscribedToEmails":true,"RegularCredits":42,"MiscItems":[{"ItemType":"/Lotus/Types/Items/x"}],"#
                .to_vec();
        blob.resize(64_000, b' ');
        blob.extend_from_slice(br#""DeathSquadable":false}"#);
        blob.resize(128_000, 0);

        // One arena so the two mappings are genuinely contiguous — adjacency is
        // what makes the walk chain them.
        let mut arena = prefix.clone();
        arena.extend_from_slice(&blob);
        let base = arena.as_ptr() as usize;
        let regions = vec![
            LinuxRegion { start: base, len: prefix.len(), executable: false },
            LinuxRegion { start: base + prefix.len(), len: blob.len(), executable: false },
        ];
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");

        let mut inventory = None;
        scan_linux_inventory_regions(&process, regions, |_, _, parsed| {
            inventory = Some(parsed);
            false
        });

        assert_eq!(inventory.expect("inventory blob is found").credits, 42);
    }

    #[test]
    fn linux_inventory_scan_finishes_before_rejecting_large_mapping() {
        let first_json = br#"{"SubscribedToEmails":true,"RegularCredits":42,"MiscItems":[],"#;
        let second_json = br#""DeathSquadable":false}"#;
        let mut first = first_json.to_vec();
        first.resize(64_000, b' ');
        let mut second = vec![0; 64 * 1024 * 1024];
        second[..second_json.len()].copy_from_slice(second_json);
        let regions = vec![
            LinuxRegion {
                start: first.as_ptr() as usize,
                len: first.len(),
                executable: false,
            },
            LinuxRegion {
                start: second.as_ptr() as usize,
                len: second.len(),
                executable: false,
            },
        ];
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");
        let mut inventory = None;

        scan_linux_inventory_regions(&process, regions, |_, _, parsed| {
            inventory = Some(parsed);
            false
        });

        assert_eq!(inventory.expect("inventory blob is found").credits, 42);
    }

    #[test]
    fn linux_inventory_scan_recovers_fields_before_start_marker() {
        let prefix =
            br#"{"RegularCredits":42,"MiscItems":[{"ItemType":"/Lotus/Test","ItemCount":1}],"#;
        let suffix = br#""SubscribedToEmails":true,"DeathSquadable":false}"#;
        let mut mapping = vec![b' '; 128_000];
        mapping[..prefix.len()].copy_from_slice(prefix);
        mapping[64_000..64_000 + suffix.len()].copy_from_slice(suffix);
        let regions = vec![
            LinuxRegion {
                start: mapping.as_ptr() as usize,
                len: 64_000,
                executable: false,
            },
            LinuxRegion {
                start: mapping.as_ptr() as usize + 64_000,
                len: 64_000,
                executable: false,
            },
        ];
        let process = LinuxProcess::open(std::process::id()).expect("current process is readable");
        let mut inventory = None;

        scan_linux_inventory_regions(&process, regions, |_, _, parsed| {
            inventory = Some(parsed);
            false
        });

        let inventory = inventory.expect("inventory blob is found");
        assert_eq!(inventory.credits, 42);
        assert_eq!(inventory.stackable_items.len(), 1);
    }

    #[test]
    fn parses_only_readable_linux_mappings() {
        let maps = "1000-2000 r--p 0 00:00 0\n2000-2800 --xp 0 00:00 0\n3000-5000 rw-p 0 00:00 0\n";
        assert_eq!(
            parse_linux_maps(maps),
            vec![
                LinuxRegion {
                    start: 0x1000,
                    len: 0x1000,
                    executable: false,
                },
                LinuxRegion {
                    start: 0x3000,
                    len: 0x2000,
                    executable: false,
                },
            ]
        );
    }

    #[test]
    fn recognizes_warframe_but_not_launcher_processes() {
        assert!(is_warframe_command(
            "Z:\\Warframe\\Warframe.x64.exe -cluster:public"
        ));
        assert!(!is_warframe_command(
            "Z:\\Warframe\\Tools\\Launcher.exe"
        ));
        assert!(!is_warframe_command("warframe-companion"));
    }
}
