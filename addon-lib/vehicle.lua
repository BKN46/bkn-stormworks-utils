-- Author: BKN
-- GitHub: https://github.com/BKN46
-- Workshop: https://steamcommunity.com/id/bkn46/myworkshopfiles/?appid=573090

--Code developed by BKN. Bilibili page: https://space.bilibili.com/522972--
--- Developed using LifeBoatAPI - Stormworks Lua plugin for VSCode - https://code.visualstudio.com/download (search "Stormworks Lua with LifeboatAPI" extension)
--- If you have any issues, please report them here: https://github.com/nameouschangey/STORMWORKS_VSCodeExtension/issues - by Nameous Changey

function registVehicle(peer_id, vehicle_id, vehicle_group_id, cost)
    if not VEHICLES then
        ---@type table<number, Vehicle>
        VEHICLES = {}
    end
    ---@class Vehicle
    local vehicle = {
        groupId = vehicle_group_id,
        vehicleId = vehicle_id,
        cost = cost,
        owner = peer_id,
        despawn = function (self)
            return server.despawnVehicleGroup(self.groupId, true)
        end,
        getGroup = function (self)
            return server.getVehicleGroup(self.groupId)
        end,
        getPos = function (self)
            local transform_matrix = server.getVehiclePos(self.vehicleId, 0, 0, 0)
            return {
                x = transform_matrix[13],
                y = transform_matrix[14],
                z = transform_matrix[15],
            }
        end,
        setPos = function (self, x, y, z)
            local transform_matrix = matrix.translation(x, y, z)
            return server.setGroupPos(self.groupId, transform_matrix)
        end,
        moveTo = function (self, x, y, z)
            local transform_matrix = matrix.translation(x, y, z)
            return server.moveGroupSafe(self.groupId, transform_matrix)
        end,
        getVehicleData = function (self)
            local data, _ = server.getVehicleData(self.vehicleId)
            return data
        end,
        getComponentData = function (self)
            local data, _ = server.getVehicleComponents(self.vehicleId)
            return data
        end,
        getSeat = function (self, seat_index)
            local data, _ = server.getVehicleSeat(self.vehicleId, seat_index)
            return data
        end,
        seatOn = function (self, player)
            local seats = self:getComponentData().components.seats
            for k, v in pairs(seats) do
                if not v.seated_id then
                    local seat_name = v.name
                    server.setSeated(player.getObjectId(), self.vehicleId, seat_name)
                    return seat_name
                end
            end
        end,
    }
    VEHICLES[vehicle_group_id] = vehicle
    return vehicle_group_id
end

function spawnAddonVehicle(component_index, x, y, z)
    local addon_index = server.getAddonIndex()
    local location_index, is_success = server.getLocationIndex(addon_index, "GARAGE")

    local transform_matrix = matrix.translation(x, y, z)
    local component_id = server.getLocationComponentData(addon_index, location_index, component_index).id
    local vehicle_group_id, is_success = server.spawnAddonVehicle(transform_matrix, addon_index, component_id)
    return vehicle_group_id
end