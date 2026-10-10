//! The position and facing of the character in a running game client (`elementclient.exe`), read-only.
//!
//! The client keeps them at `g_pGame → m_pGameRun → m_pHostPlayer → +0x3C` (`A3DCoordinate::GetPos`, the
//! last row of the absolute matrix after the vtable, class ID and name) and `+0x2C` (`GetDir`, row 2: the
//! facing the client sends to the server with `glb_CompressDirH(GetDir().x, GetDir().z)`). The global's address and both
//! member offsets differ between builds (XtremeJade +0x1C/+0x2C, ForsakenJD +0x20/+0x2C, HDN and Reborn
//! +0x20/+0x30), so [`find_chain`] reads them from the exe's code: the inlined
//! `g_pGame->GetGameRun()->GetHostPlayer()->GetPos()` loads the global, follows two pointers and reads
//! +0x3C/+0x40/+0x44 right after.

use serde::Serialize;

/// Offset of the position (x, y, z floats) in the host player object.
pub const POSITION: u32 = 0x3c;
/// Offset of the facing direction (x, y, z floats; matrix row 2) in the host player object.
pub const DIRECTION: u32 = 0x2c;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chain {
    /// Virtual address of `g_pGame` for the exe's preferred image base.
    pub global: u32,
    pub game_run: i32,
    pub host_player: i32,
    pub image_base: u32,
    /// The exe may load elsewhere (ASLR); then the global moves with the module.
    pub relocatable: bool,
}

struct Pe<'a> {
    data: &'a [u8],
    image_base: u32,
    relocatable: bool,
    /// (virtual address, virtual size, file offset, file size, flags)
    sections: Vec<(u32, u32, usize, usize, u32)>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn parse_pe(data: &[u8]) -> Result<Pe<'_>, String> {
    let bad = || "not a Windows program".to_string();
    if data.get(..2) != Some(b"MZ") {
        return Err(bad());
    }
    let pe = u32_at(data, 0x3c).ok_or_else(bad)? as usize;
    if data.get(pe..pe + 4) != Some(b"PE\0\0") {
        return Err(bad());
    }
    if u16_at(data, pe + 4) != Some(0x14c) {
        return Err("not a 32-bit game client".into());
    }
    let count = u16_at(data, pe + 6).ok_or_else(bad)? as usize;
    let optional = u16_at(data, pe + 20).ok_or_else(bad)? as usize;
    let opt = pe + 24;
    let image_base = u32_at(data, opt + 28).ok_or_else(bad)?;
    let relocatable = u16_at(data, opt + 70).ok_or_else(bad)? & 0x40 != 0;
    let mut sections = Vec::new();
    for index in 0..count {
        let at = opt + optional + index * 40;
        let field = |offset| u32_at(data, at + offset).ok_or_else(bad);
        sections.push((field(12)?, field(8)?, field(20)? as usize, field(16)? as usize, field(36)?));
    }
    Ok(Pe { data, image_base, relocatable, sections })
}

/// `mov r32, [abs32]` → (destination register, address, length).
fn load_global(code: &[u8], at: usize) -> Option<(u8, u32, usize)> {
    match *code.get(at)? {
        0xa1 => Some((0, u32_at(code, at + 1)?, 5)),
        0x8b if code.get(at + 1)? & 0xc7 == 0x05 => Some(((code[at + 1] >> 3) & 7, u32_at(code, at + 2)?, 6)),
        _ => None,
    }
}

/// `mov r32, [base + disp]` → (destination, base, displacement, length).
fn deref(code: &[u8], at: usize) -> Option<(u8, u8, i32, usize)> {
    if *code.get(at)? != 0x8b {
        return None;
    }
    let modrm = *code.get(at + 1)?;
    let (mode, rm, dst) = (modrm >> 6, modrm & 7, (modrm >> 3) & 7);
    match mode {
        _ if rm == 4 => None,
        0 if rm != 5 => Some((dst, rm, 0, 2)),
        1 => Some((dst, rm, *code.get(at + 2)? as i8 as i32, 3)),
        2 => Some((dst, rm, u32_at(code, at + 2)? as i32, 6)),
        _ => None,
    }
}

/// Reads `[reg + 0x3C/0x40/0x44]` within the next few bytes (mov, fld or movss).
fn reads_position(code: &[u8], at: usize, reg: u8) -> bool {
    (at..at + 24).any(|i| {
        let (Some(&op), Some(&modrm), Some(&disp)) = (code.get(i), code.get(i + 1), code.get(i + 2)) else { return false };
        if modrm >> 6 != 1 || modrm & 7 != reg || ![0x3c, 0x40, 0x44].contains(&disp) {
            return false;
        }
        op == 0x8b || op == 0xd9 || (op == 0x10 && i >= 2 && code[i - 2] == 0xf3 && code[i - 1] == 0x0f)
    })
}

/// Finds the position chain in a client exe's code. The winner must clearly beat every other chain.
pub fn find_chain(exe: &[u8]) -> Result<Chain, String> {
    let pe = parse_pe(exe)?;
    let in_data = |address: u32| {
        pe.sections.iter().any(|&(va, size, _, _, flags)| flags & 0x2000_0000 == 0 && address >= pe.image_base.wrapping_add(va) && address < pe.image_base.wrapping_add(va).wrapping_add(size))
    };
    let mut votes: std::collections::HashMap<(u32, i32, i32), usize> = std::collections::HashMap::new();
    for &(_, _, raw, raw_size, flags) in &pe.sections {
        if flags & 0x2000_0000 == 0 {
            continue;
        }
        let Some(code) = pe.data.get(raw..raw + raw_size) else { continue };
        for at in 0..code.len().saturating_sub(16) {
            let Some((reg, global, length)) = load_global(code, at) else { continue };
            if !in_data(global) {
                continue;
            }
            let Some((reg1, base1, game_run, length1)) = deref(code, at + length) else { continue };
            if base1 != reg {
                continue;
            }
            let Some((reg2, base2, host_player, length2)) = deref(code, at + length + length1) else { continue };
            if base2 != reg1 || !reads_position(code, at + length + length1 + length2, reg2) {
                continue;
            }
            *votes.entry((global, game_run, host_player)).or_default() += 1;
        }
    }
    let mut ranked: Vec<_> = votes.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let unrecognised = || "This game client is not recognised: its player position could not be located in elementclient.exe".to_string();
    let &((global, game_run, host_player), best) = ranked.first().ok_or_else(unrecognised)?;
    let runner_up = ranked.get(1).map_or(0, |entry| entry.1);
    if best < 4 || best < runner_up * 3 || !(0..0x1000).contains(&game_run) || !(0..0x1000).contains(&host_player) {
        return Err(unrecognised());
    }
    Ok(Chain { global, game_run, host_player, image_base: pe.image_base, relocatable: pe.relocatable })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Where the character faces (unit length); none when the matrix holds no usable direction.
    pub direction: Option<Vec3>,
}

const UNREADABLE: &str = "The game client's memory could not be read";
const NOT_PLAYING: &str = "No character is in the game yet: log in and enter the world first";

fn pointer(read: &impl Fn(u32, &mut [u8]) -> bool, address: u32) -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    if !read(address, &mut bytes) {
        return Err(UNREADABLE.into());
    }
    Ok(u32::from_le_bytes(bytes))
}

/// The address of `CECGameRun` (`[g_pGame] + game_run`), when the game is running.
pub fn game_run(chain: &Chain, module_base: u32, read: &impl Fn(u32, &mut [u8]) -> bool) -> Result<u32, String> {
    let game = pointer(read, chain.global.wrapping_sub(chain.image_base).wrapping_add(module_base))?;
    if game == 0 {
        return Err(NOT_PLAYING.into());
    }
    let run = pointer(read, game.wrapping_add(chain.game_run as u32))?;
    if run == 0 {
        return Err(NOT_PLAYING.into());
    }
    Ok(run)
}

/// Follows the chain with a memory reader (`read(address, buffer)` fills the buffer or fails).
pub fn follow(chain: &Chain, module_base: u32, read: impl Fn(u32, &mut [u8]) -> bool) -> Result<Position, String> {
    let unreadable = UNREADABLE;
    let not_playing = NOT_PLAYING;
    let run = game_run(chain, module_base, &read)?;
    let player = pointer(&read, run.wrapping_add(chain.host_player as u32))?;
    if player == 0 {
        return Err(not_playing.into());
    }
    // Row 2 (direction), its w, then row 3 (position).
    let mut bytes = [0u8; (POSITION - DIRECTION) as usize + 12];
    if !read(player.wrapping_add(DIRECTION), &mut bytes) {
        return Err(unreadable.into());
    }
    let float = |at: u32| f32::from_le_bytes(bytes[(at - DIRECTION) as usize..][..4].try_into().unwrap());
    let direction = Vec3 { x: float(DIRECTION), y: float(DIRECTION + 4), z: float(DIRECTION + 8) };
    let length = (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let direction = (length.is_finite() && (0.5..2.0).contains(&length)).then(|| Vec3 { x: direction.x / length, y: direction.y / length, z: direction.z / length });
    let position = Position { x: float(POSITION), y: float(POSITION + 4), z: float(POSITION + 8), direction };
    let sane = |value: f32, limit: f32| value.is_finite() && value.abs() < limit;
    if !(sane(position.x, 100_000.0) && sane(position.y, 20_000.0) && sane(position.z, 100_000.0)) {
        return Err("The game client returned an impossible position; this client build is not supported".into());
    }
    Ok(position)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningClient {
    pub pid: u32,
    /// Full path of the exe, when Windows tells us.
    pub path: Option<String>,
}

#[cfg(windows)]
mod process {
    use super::{find_chain, follow, Chain, Position, RunningClient};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Process32FirstW, Process32NextW, MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32, rights: u32) -> Option<Handle> {
        let handle = unsafe { OpenProcess(rights, 0, pid) };
        (!handle.is_null()).then(|| Handle(handle))
    }

    fn wide(text: &[u16]) -> String {
        String::from_utf16_lossy(&text[..text.iter().position(|&unit| unit == 0).unwrap_or(text.len())])
    }

    fn image_path(handle: &Handle) -> Option<String> {
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        (unsafe { QueryFullProcessImageNameW(handle.0, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length) } != 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]))
    }

    /// Running processes named elementclient.exe.
    pub fn running_clients() -> Vec<RunningClient> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let snapshot = Handle(snapshot);
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut out = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while more {
            if wide(&entry.szExeFile).eq_ignore_ascii_case("elementclient.exe") {
                let pid = entry.th32ProcessID;
                let path = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).and_then(|handle| image_path(&handle));
                out.push(RunningClient { pid, path });
            }
            more = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        out
    }

    /// Where the exe is loaded (only needed when it can move).
    fn module_base(pid: u32) -> Option<u32> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let snapshot = Handle(snapshot);
        let mut entry = MODULEENTRY32W { dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32, ..Default::default() };
        (unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0).then(|| entry.modBaseAddr as usize as u32)
    }

    /// Chains by exe path, size and modification time (a scan takes about a second).
    static CHAINS: Mutex<Option<HashMap<(String, u64, std::time::SystemTime), Result<Chain, String>>>> = Mutex::new(None);

    fn chain_for(path: &str) -> Result<Chain, String> {
        let meta = std::fs::metadata(path).map_err(|error| format!("{path}: {error}"))?;
        let key = (path.to_lowercase(), meta.len(), meta.modified().map_err(|error| error.to_string())?);
        if let Some(hit) = CHAINS.lock().map_err(|_| "chain cache poisoned")?.get_or_insert_with(HashMap::new).get(&key) {
            return hit.clone();
        }
        let exe = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
        let chain = find_chain(&exe);
        CHAINS.lock().map_err(|_| "chain cache poisoned")?.get_or_insert_with(HashMap::new).insert(key, chain.clone());
        chain
    }

    /// Opens a client read-only and calls `work` with its exe path, chain, module base and a memory reader.
    pub fn with_client<T>(pid: u32, work: impl FnOnce(&str, &Chain, u32, &dyn Fn(u32, &mut [u8]) -> bool) -> Result<T, String>) -> Result<T, String> {
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ)
            .ok_or("The game client could not be opened for reading (it may have closed, run as administrator, or block other programs)")?;
        let path = image_path(&handle).ok_or("The game client's program file could not be found")?;
        let chain = chain_for(&path)?;
        let base = if chain.relocatable { module_base(pid).ok_or("The game client's memory could not be read")? } else { chain.image_base };
        let read = |address: u32, buffer: &mut [u8]| {
            let mut read = 0usize;
            let ok = unsafe { ReadProcessMemory(handle.0, address as usize as *const _, buffer.as_mut_ptr().cast(), buffer.len(), &mut read) };
            ok != 0 && read == buffer.len()
        };
        work(&path, &chain, base, &read)
    }

    pub fn read_position(pid: u32) -> Result<Position, String> {
        with_client(pid, |_, chain, base, read| follow(chain, base, read))
    }
}

#[cfg(windows)]
pub use process::{read_position, running_clients, with_client};

#[cfg(not(windows))]
pub fn running_clients() -> Vec<RunningClient> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn read_position(_pid: u32) -> Result<Position, String> {
    Err("Reading the game client's position needs Windows".into())
}

#[cfg(not(windows))]
pub fn with_client<T>(_pid: u32, _work: impl FnOnce(&str, &Chain, u32, &dyn Fn(u32, &mut [u8]) -> bool) -> Result<T, String>) -> Result<T, String> {
    Err("Reading the game client needs Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_position_chain_in_real_clients() {
        let expected = [
            ("E:/Games/XtremeJade/element/elementclient.exe", 0xcf5e24, 0x1c, 0x2c),
            ("E:/Games/ForsakenJD/element/elementclient.exe", 0xe18f9c, 0x20, 0x2c),
            ("E:/Games/Elite Jade Dynasty - HDN/element/elementclient.exe", 0xfbc704, 0x20, 0x30),
            ("E:/Games/Jade Dynasty Reborn/element/elementclient.exe", 0x1130db4, 0x20, 0x30),
        ];
        for (path, global, game_run, host_player) in expected {
            let Ok(exe) = std::fs::read(path) else { continue };
            let chain = find_chain(&exe).unwrap();
            assert_eq!((chain.global, chain.game_run, chain.host_player, chain.image_base, chain.relocatable), (global, game_run, host_player, 0x40_0000, false), "{path}");
        }
        assert!(find_chain(b"MZ not really").is_err());
    }

    #[test]
    fn follows_pointers_and_rejects_empty_or_impossible_ones() {
        let chain = Chain { global: 0x0050_0000, game_run: 0x20, host_player: 0x30, image_base: 0x0040_0000, relocatable: true };
        // Loaded at 0x10000000 instead of 0x400000: the global moves with it.
        let memory = |game: u32, x: f32| {
            let mut map = std::collections::HashMap::<u32, Vec<u8>>::new();
            map.insert(0x1010_0000, game.to_le_bytes().to_vec());
            map.insert(0x2000_0020, 0x3000_0000u32.to_le_bytes().to_vec());
            map.insert(0x3000_0030, 0x4000_0000u32.to_le_bytes().to_vec());
            // Direction (0.6, 0, 0.8), w = 0, then the position.
            let floats = [0.6f32, 0.0, 0.8, 0.0, x, 12.5, -7.25];
            map.insert(0x4000_002c, floats.iter().flat_map(|value| value.to_le_bytes()).collect());
            map
        };
        let reader = |map: std::collections::HashMap<u32, Vec<u8>>| {
            move |address: u32, buffer: &mut [u8]| match map.get(&address) {
                Some(bytes) if bytes.len() >= buffer.len() => {
                    buffer.copy_from_slice(&bytes[..buffer.len()]);
                    true
                }
                _ => false,
            }
        };
        assert_eq!(follow(&chain, 0x1000_0000, reader(memory(0x2000_0000, -401.25))).unwrap(), Position { x: -401.25, y: 12.5, z: -7.25, direction: Some(Vec3 { x: 0.6, y: 0.0, z: 0.8 }) });
        assert!(follow(&chain, 0x1000_0000, reader(memory(0, 1.0))).unwrap_err().contains("log in"));
        assert!(follow(&chain, 0x1000_0000, reader(memory(0x2000_0000, f32::NAN))).unwrap_err().contains("impossible"));
        assert!(follow(&chain, 0x1000_0000, reader(std::collections::HashMap::new())).unwrap_err().contains("could not be read"));
    }
}
