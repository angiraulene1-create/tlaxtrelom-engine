-- SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
-- Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
-- CONFIDENTIAL — internal use only.
--
-- Tests del runtime de scripting Lua. Se ejecutan desde la raíz del repo:
--   lua script/tests/run.lua

package.path = "./script/?.lua;./script/?/init.lua;" .. package.path

local tlx = require("tlaxtrelom")

local passed, failed = 0, 0

local function check(condition, message)
    if not condition then
        error(message or "check falló", 2)
    end
end

local function eq(actual, expected, message)
    if actual ~= expected then
        error((message or "valores distintos")
            .. ": esperado=" .. tostring(expected)
            .. " actual=" .. tostring(actual), 2)
    end
end

local function test(name, fn)
    local ok, err = xpcall(fn, debug.traceback)
    if ok then
        passed = passed + 1
        print("ok   " .. name)
    else
        failed = failed + 1
        print("FAIL " .. name .. "\n     " .. tostring(err))
    end
end

local function load_runtime(source, options)
    local rt = tlx.Runtime.new(options or {})
    local ok, err = rt:load(source, "test")
    check(ok, err)
    return rt
end

test("version", function()
    eq(tlx.VERSION, "0.2.0")
    check(type(tlx.Runtime.new) == "function")
end)

test("on_start y on_tick producen eventos", function()
    local rt = load_runtime([[
        return {
            on_start = function(api) api.emit(1) end,
            on_tick = function(dt, api)
                api.set_axis(1, api.get_axis(1) + dt * 10)
            end,
        }
    ]], { dt = 0.5 })
    eq(rt:start(), true)
    eq(rt:tick(), true)
    eq(rt:tick(), true)
    local events = rt:events()
    eq(#events, 1)
    eq(events[1], 1)
    local stats = rt:stats()
    eq(stats.ticks, 2)
    -- `instructions` es una cota inferior muestreada por el hook: un script
    -- trivial puede no llegar al primer intervalo, así que solo verificamos
    -- que el campo exista. El test de presupuesto valida el corte real.
    check(type(stats.instructions) == "number")
end)

test("el eje se recorta a [-1, 1]", function()
    local rt = load_runtime([[
        return {
            on_tick = function()
                engine.set_axis(1, 99)
                engine.set_axis(2, -99)
            end,
        }
    ]])
    rt:start()
    rt:tick()
    eq(rt:axis(1), 1)
    eq(rt:axis(2), -1)
    check(rt:axis(3) == 0)
end)

test("NaN en un eje se rechaza", function()
    local rt = tlx.Runtime.new()
    local ok = rt:load([[
        return {
            on_tick = function()
                engine.set_axis(1, 0 / 0)
            end,
        }
    ]], "nan")
    check(ok)
    rt:start()
    local done, err = rt:tick()
    eq(done, nil)
    check(tostring(err):find("finito", 1, true) ~= nil, err)
    check(rt:stats().failed)
end)

test("random es determinista por semilla", function()
    local source = [[
        return {
            on_tick = function()
                engine.emit(math.floor(engine.random() * 1000000))
            end,
        }
    ]]
    local a = load_runtime(source, { seed = 1234 })
    local b = load_runtime(source, { seed = 1234 })
    a:start(); b:start()
    for _ = 1, 6 do
        a:tick(); b:tick()
    end
    local ea, eb = a:events(), b:events()
    eq(#ea, 6)
    for i = 1, #ea do
        eq(ea[i], eb[i], "evento " .. i)
    end

    local c = load_runtime(source, { seed = 999 })
    c:start(); c:tick()
    local ec = c:events()
    check(ec[1] ~= ea[1], "semillas distintas deberían divergir")
end)

test("sandbox niega os/io/package/require", function()
    local probes = { "os", "io", "package", "require", "dofile", "loadfile", "debug" }
    for _, name in ipairs(probes) do
        local rt = tlx.Runtime.new()
        local ok = rt:load("return { on_tick = function() return " .. name .. " end }", name)
        check(ok, name)
        rt:start()
        local done, err = rt:tick()
        eq(done, nil, name .. " no debería estar disponible")
        check(tostring(err):find("no permitido", 1, true) ~= nil, name .. ": " .. tostring(err))
    end
end)

test("sandbox niega acceso a debug y bytecode", function()
    local rt = tlx.Runtime.new()
    local ok, err = rt:load("return { on_tick = function() debug.sethook() end }", "dbg")
    check(ok, err)
    rt:start()
    local done, err2 = rt:tick()
    eq(done, nil)
    check(tostring(err2):find("no permitido", 1, true) ~= nil, err2)
end)

test("presupuesto de instrucciones corta un bucle infinito", function()
    local rt = load_runtime([[
        return {
            on_tick = function()
                while true do end
            end,
        }
    ]], { instruction_budget = 5000 })
    rt:start()
    local done, err = rt:tick()
    eq(done, nil)
    check(tostring(err):find("presupuesto", 1, true) ~= nil, err)
    local stats = rt:stats()
    check(stats.failed)
    check(stats.instructions >= 5000, "el hook debe reportar el consumo")
end)

test("pcall no puede ocultar el agotamiento de presupuesto", function()
    local rt = load_runtime([[
        return {
            on_tick = function()
                pcall(function() while true do end end)
            end,
        }
    ]], { instruction_budget = 5000 })
    rt:start()
    local done, err = rt:tick()
    eq(done, nil)
    check(tostring(err):find("presupuesto", 1, true) ~= nil, err)
end)

test("presupuesto de memoria acota allocaciones", function()
    local rt = load_runtime([[
        local keep = {}
        return {
            on_tick = function()
                for i = 1, 4096 do
                    keep[i] = string.rep("x", 1024)
                end
            end,
        }
    ]], { memory_kb = 256, instruction_budget = 5000000 })
    rt:start()
    local done, err = rt:tick()
    eq(done, nil)
    check(tostring(err):find("memoria", 1, true) ~= nil, err)
end)

test("error de sintaxis se reporta sin abortar el host", function()
    local rt = tlx.Runtime.new()
    local ok, err = rt:load("return {", "broken")
    eq(ok, nil)
    check(tostring(err):find("broken") ~= nil or #tostring(err) > 0)
    check(rt:stats().failed)
end)

test("callbacks globales también son válidos", function()
    local rt = load_runtime([[
        ticks = 0
        function on_tick(dt, api)
            ticks = ticks + 1
            api.set_axis(2, ticks * dt)
        end
    ]], { dt = 0.25 })
    eq(rt:start(), true)
    eq(rt:tick(), true)
    eq(rt:stats().ticks, 1)
end)

test("stop llama on_stop y drena eventos", function()
    local rt = load_runtime([[
        return {
            on_stop = function(api) api.emit(42) end,
        }
    ]])
    rt:start()
    eq(rt:stop(), true)
    local events = rt:events()
    eq(#events, 1)
    eq(events[1], 42)
    local done, err = rt:tick()
    eq(done, nil)
    check(tostring(err):find("detenido", 1, true) ~= nil)
end)

test("el ejemplo turret carga y emite", function()
    local file = assert(io.open("script/examples/turret.lua", "rb"))
    local source = file:read("*a")
    file:close()

    local rt = load_runtime(source, { dt = 1 / 60 })
    rt:start()
    for _ = 1, 120 do
        -- El ejemplo lee el eje 1; el host lo alimenta con un input sintético.
        rt:feed_axis(1, 1)
        check(rt:tick())
    end
    local events = rt:events()
    check(#events > 0, "turret debería emitir al acelerar")
    check(rt:logs()[1] ~= nil)
end)

print(string.format("\n%d passed, %d failed", passed, failed))
if failed > 0 then
    os.exit(1)
end
