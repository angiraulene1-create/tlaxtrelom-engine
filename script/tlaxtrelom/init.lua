-- SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
-- Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
-- CONFIDENTIAL — internal use only.
--
-- Runtime de scripting de Tlaxtrelom Engine (Lua 5.4+).
--
-- El scripting es *sandbox*: no hay `os`, `io`, `package`, `require`, `debug`
-- ni carga de bytecode. Cada script recibe una API pequeña y determinista; el
-- host controla el presupuesto de instrucciones, la memoria y los eventos.
--
-- El runtime del motor (Rust) no interpreta Lua en el camino caliente. Los
-- scripts se compilan/cargan al entrar en un nivel y producen eventos e intents
-- que el host consume por lote, igual que cualquier otro dispositivo de entrada.

local M = {}

local DEFAULT_DT = 1 / 120
local DEFAULT_BUDGET = 50000
local DEFAULT_MEMORY_KB = 16 * 1024
local MAX_EVENTS = 4096
local MAX_LOGS = 256

local function shallow_copy(source)
    local copy = {}
    for key, value in pairs(source) do
        copy[key] = value
    end
    return copy
end

local function finite(value)
    return type(value) == "number" and value == value and value ~= math.huge and value ~= -math.huge
end

local function check_integer(value, label)
    if type(value) ~= "number" or value ~= math.floor(value) then
        error(label .. " debe ser entero", 3)
    end
    return value
end

local Runtime = {}
Runtime.__index = Runtime

function Runtime.new(options)
    options = options or {}
    if type(options) ~= "table" then
        error("Runtime.new espera una tabla de opciones", 2)
    end

    local dt = options.dt or DEFAULT_DT
    local budget = options.instruction_budget or DEFAULT_BUDGET
    local memory_kb = options.memory_kb or DEFAULT_MEMORY_KB
    local seed = options.seed or 0x5EED

    if not finite(dt) or dt <= 0 or dt > 1 then
        error("dt debe estar en (0, 1]", 2)
    end
    check_integer(budget, "instruction_budget")
    check_integer(memory_kb, "memory_kb")
    check_integer(seed, "seed")
    if budget < 1000 then
        error("instruction_budget debe ser >= 1000", 2)
    end
    if memory_kb < 64 then
        error("memory_kb debe ser >= 64", 2)
    end

    local self = setmetatable({}, Runtime)
    self.dt = dt
    self.budget = budget
    self.memory_kb = memory_kb
    self.max_seed = 2147483647
    self.seed = math.abs(seed) % self.max_seed
    if self.seed == 0 then
        self.seed = 1
    end
    self.state = {}
    self.axes = { 0, 0, 0, 0, 0, 0, 0, 0 }
    self._events = {}
    self._logs = {}
    self._ticks = 0
    self._instructions = 0
    self._started = false
    self._stopped = false
    self._failed = false
    self.last_error = nil
    self._callbacks = nil
    self._env = nil
    self._api = self:_make_api()
    return self
end

function Runtime:_random()
    -- LCG determinista de 32 bits (Numerical Recipes). No usa math.random:
    -- dos hosts con la misma semilla deben producir la misma secuencia.
    self.seed = (1664525 * self.seed + 1013904223) % 4294967296
    return self.seed / 4294967296
end

function Runtime:_set_axis(index, value)
    check_integer(index, "set_axis.index")
    if index < 1 or index > 8 then
        error("set_axis.index debe estar en 1..8", 3)
    end
    if not finite(value) then
        error("set_axis.value debe ser finito", 3)
    end
    self.axes[index] = math.max(-1, math.min(1, value))
end

function Runtime:_get_axis(index)
    check_integer(index, "get_axis.index")
    if index < 1 or index > 8 then
        error("get_axis.index debe estar en 1..8", 3)
    end
    return self.axes[index]
end

function Runtime:feed_axis(index, value)
    self:_set_axis(index, value)
end

function Runtime:axis(index)
    return self:_get_axis(index)
end

function Runtime:_make_api()
    local self = self
    return {
        state = self.state,
        emit = function(code)
            check_integer(code, "emit.code")
            if code < 0 or code > 0x7fffffff then
                error("emit.code fuera de rango", 2)
            end
            if #self._events >= MAX_EVENTS then
                error("cola de eventos llena (backpressure)", 2)
            end
            self._events[#self._events + 1] = code
        end,
        set_axis = function(index, value)
            self:_set_axis(index, value)
        end,
        get_axis = function(index)
            return self:_get_axis(index)
        end,
        time = function()
            return self._ticks * self.dt
        end,
        random = function()
            return self:_random()
        end,
        randomseed = function(seed)
            check_integer(seed, "randomseed")
            self.seed = math.abs(seed) % self.max_seed
            if self.seed == 0 then
                self.seed = 1
            end
        end,
        log = function(message)
            if #self._logs >= MAX_LOGS then
                table.remove(self._logs, 1)
            end
            self._logs[#self._logs + 1] = tostring(message)
        end,
    }
end

function Runtime:_make_env()
    local function guarded_pcall(fn, ...)
        local ok, err = pcall(fn, ...)
        if not ok and tostring(err):find("presupuesto", 1, true) then
            -- Un script no puede capturar el agotamiento de presupuesto y
            -- seguir ejecutando: eso convertiría el sandbox en un DoS.
            error(err, 0)
        end
        return ok, err
    end

    local function guarded_xpcall(fn, handler, ...)
        local ok, err = xpcall(fn, handler, ...)
        if not ok and tostring(err):find("presupuesto", 1, true) then
            error(err, 0)
        end
        return ok, err
    end

    local base = {
        assert = assert,
        error = error,
        ipairs = ipairs,
        pairs = pairs,
        next = next,
        select = select,
        tonumber = tonumber,
        tostring = tostring,
        type = type,
        pcall = guarded_pcall,
        xpcall = guarded_xpcall,
        rawget = rawget,
        rawset = rawset,
        rawequal = rawequal,
        setmetatable = setmetatable,
        getmetatable = getmetatable,
        table = shallow_copy(table),
        string = shallow_copy(string),
        math = shallow_copy(math),
        engine = self._api,
    }

    if utf8 then
        base.utf8 = shallow_copy(utf8)
    end

    -- La versión del sandbox de math no usa el estado global de RNG.
    base.math.random = function()
        return self:_random()
    end
    base.math.randomseed = function(seed)
        self._api.randomseed(seed)
    end

    base._G = base
    setmetatable(base, {
        __index = function(_, key)
            error("global no permitido en sandbox: " .. tostring(key), 2)
        end,
    })
    return base
end

function Runtime:_with_budget(fn)
    local interval = 500
    local spent = 0
    debug.sethook(function()
        spent = spent + interval
        if spent > self.budget then
            error("presupuesto de instrucciones excedido", 0)
        end
        if collectgarbage("count") > self.memory_kb then
            error("presupuesto de memoria excedido", 0)
        end
    end, "", interval)

    local ok, result = pcall(fn)
    debug.sethook()
    self._instructions = self._instructions + spent
    if not ok then
        return nil, tostring(result)
    end
    return result
end

function Runtime:load(source, name)
    if type(source) ~= "string" then
        return nil, "el script debe ser texto"
    end
    if self._started then
        return nil, "no se puede recargar un script en ejecución"
    end

    self._env = self:_make_env()
    self._events = {}
    self._logs = {}
    self._ticks = 0
    self._instructions = 0
    self._failed = false
    self.last_error = nil

    local chunk, compile_error = load(source, name or "tlaxtrelom-script", "t", self._env)
    if not chunk then
        self._failed = true
        self.last_error = tostring(compile_error)
        return nil, self.last_error
    end

    local result, runtime_error = self:_with_budget(function()
        return chunk(self._api)
    end)
    if runtime_error then
        self:_fail(runtime_error)
        return nil, runtime_error
    end

    local callbacks = result
    if callbacks == nil then
        -- También se aceptan funciones globales `on_start`, `on_tick`, `on_stop`.
        callbacks = {
            on_start = rawget(self._env, "on_start"),
            on_tick = rawget(self._env, "on_tick"),
            on_stop = rawget(self._env, "on_stop"),
        }
    end
    if type(callbacks) ~= "table" then
        self:_fail("el script debe devolver una tabla de callbacks")
        return nil, self.last_error
    end
    for _, key in ipairs({ "on_start", "on_tick", "on_stop" }) do
        if callbacks[key] ~= nil and type(callbacks[key]) ~= "function" then
            self:_fail(key .. " debe ser función")
            return nil, self.last_error
        end
    end

    self._callbacks = callbacks
    return true
end

function Runtime:_fail(message)
    self._failed = true
    self.last_error = tostring(message)
end

function Runtime:start()
    if self._failed then
        return nil, self.last_error
    end
    if self._callbacks == nil then
        return nil, "no hay script cargado"
    end
    if self._started then
        return nil, "el script ya está iniciado"
    end
    self._started = true
    if self._callbacks.on_start then
        local _, err = self:_with_budget(function()
            return self._callbacks.on_start(self._api)
        end)
        if err then
            self:_fail(err)
            return nil, err
        end
    end
    return true
end

function Runtime:tick()
    if self._failed then
        return nil, self.last_error
    end
    if not self._started then
        return nil, "el script no está iniciado"
    end
    if self._stopped then
        return nil, "el script está detenido"
    end

    if self._callbacks.on_tick then
        local _, err = self:_with_budget(function()
            return self._callbacks.on_tick(self.dt, self._api)
        end)
        if err then
            self:_fail(err)
            return nil, err
        end
        self._ticks = self._ticks + 1
        return true
    end
    self._ticks = self._ticks + 1
    return true
end

function Runtime:stop()
    if self._stopped then
        return true
    end
    self._stopped = true
    if self._callbacks and self._callbacks.on_stop then
        local _, err = self:_with_budget(function()
            return self._callbacks.on_stop(self._api)
        end)
        if err then
            self:_fail(err)
            return nil, err
        end
    end
    return true
end

function Runtime:events()
    local copy = {}
    for index, value in ipairs(self._events) do
        copy[index] = value
    end
    self._events = {}
    return copy
end

function Runtime:logs()
    return shallow_copy(self._logs)
end

function Runtime:stats()
    return {
        ticks = self._ticks,
        instructions = self._instructions,
        events = #self._events,
        logs = #self._logs,
        started = self._started,
        stopped = self._stopped,
        failed = self._failed,
        seed = self.seed,
        last_error = self.last_error,
    }
end

M.Runtime = Runtime
M.new = Runtime.new
M.VERSION = "0.2.0"
return M
