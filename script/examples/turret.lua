-- SPDX-License-Identifier: LicenseRef-Tlaxtrelom-Proprietary
-- Copyright (c) 2026 Tlaxtrelom Engine. All rights reserved.
-- CONFIDENTIAL — internal use only.
--
-- Ejemplo de script de gameplay. El runtime no lo interpreta en el hilo de
-- render: el host lo carga al entrar en el nivel y consume sus eventos por lote.

local speed = 0.0
local cooldown = 0.0

return {
    on_start = function(api)
        api.log("turret: iniciado")
    end,

    on_tick = function(dt, api)
        local forward = api.get_axis(1)
        local turn = api.get_axis(3)
        speed = math.max(-1, math.min(1, speed + forward * dt * 2))

        if turn ~= 0 then
            api.set_axis(4, turn)
        end

        cooldown = math.max(0, cooldown - dt)
        if math.abs(speed) > 0.8 and cooldown == 0 then
            api.emit(0x1001)
            cooldown = 0.25
        end
    end,

    on_stop = function(api)
        api.log("turret: detenido")
    end,
}
