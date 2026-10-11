// The parts of fengari-web (the browser build of fengari, a Lua 5.3 VM in JavaScript) the Lua viewers use; it ships no types.
declare module "fengari-web" {
  type State = unknown;
  export const lua: {
    LUA_MULTRET: number;
    lua_pcall(L: State, args: number, results: number, handler: number): number;
    lua_tostring(L: State, index: number): Uint8Array | null;
    lua_pop(L: State, count: number): void;
    lua_getglobal(L: State, name: Uint8Array): number;
    lua_close(L: State): void;
  };
  export const lauxlib: {
    luaL_newstate(): State;
    luaL_loadbuffer(L: State, code: Uint8Array, size: number | null, name: Uint8Array): number;
  };
  export const lualib: { luaL_openlibs(L: State): void };
  export function to_luastring(text: string): Uint8Array;
  export function to_jsstring(bytes: Uint8Array): string;
}
