import { lauxlib, lua, lualib, to_jsstring, to_luastring } from "fengari-web";

/** One way a lottery pays out (the six numbers `LotteryCashing` returns) and how likely it is. */
export interface LotteryOutcome {
  tier: number;
  money: number;
  item: number;
  count: number;
  bind: number;
  period: number;
  chance: number;
}

export interface LotteryResult {
  id: number;
  dice: number;
  /** Every branch walked (otherwise sampled). */
  exact: boolean;
  /** Leaves walked or samples drawn. */
  runs: number;
  /** Chance of each value per drawn digit (position 1 first). */
  digits: { value: number; chance: number }[][];
  outcomes: LotteryOutcome[];
  /** What the server's start-up test (`item_lottery::CheckScript`) would report: the item is then not created. */
  error: string | null;
}

// Runs in the sandbox after the script: the random pickers become branch points the driver walks.
const DRIVER = String.raw`
local __forced, __calls, __pos, __sampling, __continuous = {}, {}, 0, false, false
local __random = math.random

local function __pick(weights)
  __pos = __pos + 1
  local w, total = {}, 0
  for i = 1, #weights do
    local v = tonumber(weights[i]) or 0
    if v ~= v or v < 0 then v = 0 end
    w[i] = v
    total = total + v
  end
  __calls[__pos] = w
  if __sampling then
    if total <= 0 then return 1 end
    local r, acc = __random() * total, 0
    for i = 1, #w do acc = acc + w[i]; if r <= acc and w[i] > 0 then return i end end
    return #w
  end
  local c = __forced[__pos]
  if c == nil then
    c = 1
    for i = 1, #w do if w[i] > 0 then c = i break end end
  end
  return c
end

-- Every "*_Get_RanPoint(weights)" picks by weight (all zero: the first).
for name, value in pairs(_G) do
  if type(value) == "function" and string.find(name, "_Get_RanPoint$") then _G[name] = __pick end
end
math.random = function(m, n)
  if m == nil then __continuous = true; return __random() end
  if n == nil then m, n = 1, m end
  if __sampling or n - m > 200 then return __random(m, n) end
  local w = {}
  for i = m, n do w[#w + 1] = 1 end
  return m - 1 + __pick(w)
end

local function __play(id, dice)
  local tickets = {}
  for i = 1, dice do
    local previous = {}
    for k = 1, i - 1 do previous[k] = tickets[k] end
    local value = LotterySelectNumber(id, dice, previous)
    if type(value) ~= "number" then error("digit " .. i .. " is not a number (" .. tostring(value) .. ")", 0) end
    tickets[i] = math.floor(value) % 256
  end
  local result = { LotteryCashing(id, tickets) }
  if #result < 6 then error("the cashing returns " .. #result .. " value(s) instead of 6", 0) end
  for i = 1, 6 do
    if type(result[i]) ~= "number" then error("cashing value " .. i .. " is not a number", 0) end
  end
  return tickets, result
end

function __explore(id, dice, max_leaves, samples)
  local out, digits, leaves = {}, {}, 0
  local function record(tickets, result, p)
    local key = string.format("%d,%d,%d,%d,%d,%d", result[1], result[2], result[3], result[4], result[5], result[6])
    out[key] = (out[key] or 0) + p
    for i = 1, #tickets do
      digits[i] = digits[i] or {}
      digits[i][tickets[i]] = (digits[i][tickets[i]] or 0) + p
    end
  end
  local function finish(mode, count)
    local lines = { "S\t" .. mode .. "\t" .. count }
    for key, p in pairs(out) do lines[#lines + 1] = "O\t" .. key .. "\t" .. string.format("%.17g", p) end
    for i, values in pairs(digits) do
      for value, p in pairs(values) do lines[#lines + 1] = "D\t" .. i .. "\t" .. value .. "\t" .. string.format("%.17g", p) end
    end
    return table.concat(lines, "\n")
  end
  -- The server's own test: one draw with the first choices.
  __sampling, __continuous, __forced, __calls, __pos = false, false, {}, {}, 0
  local ok, err = pcall(__play, id, dice)
  if not ok then return "E\t" .. tostring(err) end
  -- Walk every branch.
  local stack = { { forced = {}, p = 1 } }
  while #stack > 0 and not __continuous do
    local node = table.remove(stack)
    __forced, __calls, __pos = node.forced, {}, 0
    local good, tickets, result = pcall(__play, id, dice)
    if not good then return "E\t" .. tostring(tickets) end
    if #__calls > #node.forced then
      local k = #node.forced + 1
      local w, total = __calls[k], 0
      for i = 1, #w do total = total + w[i] end
      if total <= 0 then
        local f = {}; for i = 1, #node.forced do f[i] = node.forced[i] end; f[k] = 1
        stack[#stack + 1] = { forced = f, p = node.p }
      else
        for j = #w, 1, -1 do
          if w[j] > 0 then
            local f = {}; for i = 1, #node.forced do f[i] = node.forced[i] end; f[k] = j
            stack[#stack + 1] = { forced = f, p = node.p * w[j] / total }
          end
        end
      end
    else
      leaves = leaves + 1
      record(tickets, result, node.p)
      if leaves > max_leaves then break end
    end
  end
  if not __continuous and #stack == 0 then return finish("exact", leaves) end
  -- Too many branches, or plain math.random(): draw samples instead.
  out, digits, __sampling = {}, {}, true
  for s = 1, samples do
    __calls, __pos = {}, 0
    local good, tickets, result = pcall(__play, id, dice)
    if not good then return "E\t" .. tostring(tickets) end
    record(tickets, result, 1 / samples)
  end
  return finish("sampled", samples)
end
`;

// Lua 5.1 names the server's scripts use (fengari is Lua 5.3).
const COMPAT = `table.getn = table.getn or function(t) return #t end
unpack = unpack or table.unpack
math.mod = math.mod or math.fmod
math.pow = math.pow or function(a, b) return a ^ b end
string.gfind = string.gfind or string.gmatch
loadstring = loadstring or load
table.setn = table.setn or function() end`;

/** A Lua state with a lottery script loaded, for exact odds. */
export class LotterySandbox {
  private L: unknown;
  /** Set when the script itself fails to load (gs then stops with -11). */
  readonly loadError: string | null = null;

  constructor(text: string, name = "lottery.lua") {
    this.L = lauxlib.luaL_newstate();
    lualib.luaL_openlibs(this.L);
    try {
      this.run(COMPAT, "compat");
      this.run(text, name);
      this.run(DRIVER, "driver");
    } catch (problem) {
      (this as { loadError: string | null }).loadError = String((problem as Error).message ?? problem);
    }
  }

  private run(code: string, name: string) {
    const L = this.L;
    if (lauxlib.luaL_loadbuffer(L, to_luastring(code), null, to_luastring(name)) || lua.lua_pcall(L, 0, lua.LUA_MULTRET, 0)) {
      const message = to_jsstring(lua.lua_tostring(L, -1) ?? to_luastring("error"));
      lua.lua_pop(L, 1);
      throw new Error(message);
    }
  }

  private call(code: string): string {
    this.run(`__out = ${code}`, "call");
    lua.lua_getglobal(this.L, to_luastring("__out"));
    const text = to_jsstring(lua.lua_tostring(this.L, -1) ?? to_luastring(""));
    lua.lua_pop(this.L, 1);
    return text;
  }

  /** The odds of a lottery item with `dice` digits (its elements.data `dice_count`). */
  evaluate(id: number, dice: number, maxLeaves = 20000, samples = 20000): LotteryResult {
    const result: LotteryResult = { id, dice, exact: false, runs: 0, digits: [], outcomes: [], error: null };
    if (this.loadError) { result.error = this.loadError; return result; }
    let text: string;
    try {
      text = this.call(`__explore(${Math.trunc(id)}, ${Math.max(1, Math.trunc(dice))}, ${maxLeaves}, ${samples})`);
    } catch (problem) {
      result.error = String((problem as Error).message ?? problem);
      return result;
    }
    for (const line of text.split("\n")) {
      const parts = line.split("\t");
      if (parts[0] === "E") result.error = parts.slice(1).join("\t");
      else if (parts[0] === "S") { result.exact = parts[1] === "exact"; result.runs = Number(parts[2]); }
      else if (parts[0] === "O") {
        const [tier, money, item, count, bind, period] = parts[1].split(",").map(Number);
        result.outcomes.push({ tier, money, item, count, bind, period, chance: Number(parts[2]) });
      } else if (parts[0] === "D") {
        const position = Number(parts[1]) - 1;
        (result.digits[position] ??= []).push({ value: Number(parts[2]), chance: Number(parts[3]) });
      }
    }
    result.outcomes.sort((a, b) => a.tier - b.tier || b.chance - a.chance);
    for (const values of result.digits) values?.sort((a, b) => a.value - b.value);
    return result;
  }

  dispose() {
    lua.lua_close(this.L);
  }
}
