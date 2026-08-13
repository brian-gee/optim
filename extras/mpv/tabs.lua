-- Tab-style playlist control for the shared optim player window.
--
-- Keyboard:
--   TAB              open/close the tab menu
--     UP/DOWN (j/k)  move selection        ENTER  switch to selected
--     X              close selected tab    D      detach selected
--     1-9            jump to that tab      ESC    close menu
--   Ctrl+TAB / Ctrl+Shift+TAB   next / previous tab (menu closed)
--   D                            detach current tab (menu closed)
--
-- Mouse:
--   TAB menu open:  hover highlights - left-click switches -
--                   right-click closes that tab - middle-click detaches -
--                   wheel moves selection - click outside closes the menu
--   Anywhere:       Ctrl+wheel cycles tabs
--   (the stock OSC's |< >| buttons on hover also page the playlist)

local RES_X, RES_Y = 1280, 720
local MENU_X = 24
local TITLE_Y = 22
local ROW0_Y = 64
local ROW_H = 30

local menu_open = false
local sel = 1
local overlay = mp.create_osd_overlay("ass-events")
overlay.res_x, overlay.res_y = RES_X, RES_Y
local idle_timer = nil

local function playlist_count()
    return mp.get_property_number("playlist-count", 0)
end

local function entry_label(i)
    local title = mp.get_property(string.format("playlist/%d/title", i - 1))
    if title and #title > 0 then return title end
    local fname = mp.get_property(string.format("playlist/%d/filename", i - 1)) or "?"
    fname = fname:gsub("[?#].*$", "")
    local tail = fname:match("([^/\\]+)/*$") or fname
    tail = tail:gsub("^%d+%-", "") -- strip optim's timestamp prefix
    if #tail > 56 then tail = tail:sub(1, 53) .. "..." end
    return tail
end

local function ass_escape(s)
    return s:gsub("[\\{}]", "_")
end

local function detach(index0)
    local path = mp.get_property(string.format("playlist/%d/filename", index0))
    if not path then return end
    mp.commandv("run", "mpv", "--input-ipc-server=", path)
    if playlist_count() > 1 then
        mp.commandv("playlist-remove", tostring(index0))
    end
end

local function clamp_sel()
    local count = playlist_count()
    if sel > count then sel = count end
    if sel < 1 then sel = 1 end
end

local function render()
    clamp_sel()
    local count = playlist_count()
    local pos = mp.get_property_number("playlist-pos-1", 1)
    local ev = {
        string.format(
            "{\\pos(%d,%d)\\an7\\fs24\\bord1.5\\b1}tabs{\\b0\\fs16\\alpha&H70&}   click switch · right-click close · wheel move · ESC",
            MENU_X, TITLE_Y),
    }
    for i = 1, count do
        local y = ROW0_Y + (i - 1) * ROW_H
        local marker = (i == pos) and "▶ " or "    "
        local label = ass_escape(entry_label(i))
        local line
        if i == sel then
            line = string.format(
                "{\\pos(%d,%d)\\an7\\fs21\\bord1.5\\1c&HFEBEB4&}%s%d  %s",
                MENU_X, y, marker, i, label)
        else
            line = string.format(
                "{\\pos(%d,%d)\\an7\\fs21\\bord1.5\\alpha&H30&}%s%d  %s",
                MENU_X, y, marker, i, label)
        end
        ev[#ev + 1] = line
    end
    overlay.data = table.concat(ev, "\n")
    overlay:update()
end

-- Window pixel -> overlay row index (nil when not over a row).
local function row_at_mouse()
    local mouse = mp.get_property_native("mouse-pos")
    local dims = mp.get_property_native("osd-dimensions")
    if not mouse or not dims or dims.h == 0 then return nil end
    local y = mouse.y * RES_Y / dims.h
    local x = mouse.x * RES_X / dims.w
    if x < MENU_X - 10 or x > RES_X * 0.7 then return nil end
    local i = math.floor((y - ROW0_Y + ROW_H * 0.5) / ROW_H) + 1
    if i >= 1 and i <= playlist_count() then return i end
    return nil
end

function close_menu()
    if not menu_open then return end
    menu_open = false
    if idle_timer then idle_timer:kill(); idle_timer = nil end
    for i = 1, 40 do
        mp.remove_key_binding("tabmenu" .. i)
    end
    overlay.data = ""
    overlay:update()
end

local function poke_idle_timer()
    if idle_timer then idle_timer:kill() end
    idle_timer = mp.add_timeout(8, function() close_menu() end)
end

local function open_menu()
    menu_open = true
    sel = mp.get_property_number("playlist-pos-1", 1)

    local binds = {
        { "UP", function() sel = sel - 1; render(); poke_idle_timer() end, { repeatable = true } },
        { "DOWN", function() sel = sel + 1; render(); poke_idle_timer() end, { repeatable = true } },
        { "k", function() sel = sel - 1; render(); poke_idle_timer() end, { repeatable = true } },
        { "j", function() sel = sel + 1; render(); poke_idle_timer() end, { repeatable = true } },
        { "WHEEL_UP", function() sel = sel - 1; render(); poke_idle_timer() end },
        { "WHEEL_DOWN", function() sel = sel + 1; render(); poke_idle_timer() end },
        { "ENTER", function()
              clamp_sel()
              mp.set_property_number("playlist-pos-1", sel)
              close_menu()
          end },
        { "x", function()
              if playlist_count() > 1 then
                  mp.commandv("playlist-remove", tostring(sel - 1))
              end
              render(); poke_idle_timer()
          end },
        { "d", function() detach(sel - 1); render(); poke_idle_timer() end },
        { "ESC", function() close_menu() end },
        { "MOUSE_MOVE", function()
              local row = row_at_mouse()
              if row and row ~= sel then
                  sel = row
                  render()
              end
              poke_idle_timer()
          end },
        { "MBTN_LEFT", function()
              local row = row_at_mouse()
              if row then
                  mp.set_property_number("playlist-pos-1", row)
              end
              close_menu() -- click outside rows also dismisses
          end },
        { "MBTN_RIGHT", function()
              local row = row_at_mouse()
              if row and playlist_count() > 1 then
                  mp.commandv("playlist-remove", tostring(row - 1))
              end
              render(); poke_idle_timer()
          end },
        { "MBTN_MID", function()
              local row = row_at_mouse()
              if row then detach(row - 1) end
              render(); poke_idle_timer()
          end },
    }
    for n = 1, 9 do
        binds[#binds + 1] = { tostring(n), function()
            if n <= playlist_count() then
                mp.set_property_number("playlist-pos-1", n)
                close_menu()
            end
        end }
    end
    for i, b in ipairs(binds) do
        mp.add_forced_key_binding(b[1], "tabmenu" .. i, b[2], b[3])
    end
    render()
    poke_idle_timer()
end

mp.add_key_binding("TAB", "tab-menu", function()
    if menu_open then close_menu() else open_menu() end
end)

-- Menu-closed shortcuts.
local function cycle(dir)
    mp.commandv(dir == 1 and "playlist-next" or "playlist-prev", "weak")
    mp.osd_message(string.format("tab %d/%d",
        mp.get_property_number("playlist-pos-1", 1), playlist_count()))
end
mp.add_key_binding("Ctrl+TAB", "tab-next", function() cycle(1) end)
mp.add_key_binding("Ctrl+Shift+TAB", "tab-prev", function() cycle(-1) end)
mp.add_key_binding("Ctrl+WHEEL_DOWN", "tab-next-wheel", function() cycle(1) end)
mp.add_key_binding("Ctrl+WHEEL_UP", "tab-prev-wheel", function() cycle(-1) end)

mp.add_key_binding("D", "detach-current", function()
    detach(mp.get_property_number("playlist-pos", 0))
    mp.osd_message("detached to its own window")
end)

-- Keep the menu fresh while optim appends.
mp.observe_property("playlist-count", "number", function(_, count)
    if menu_open then
        render()
    elseif count and count > 1 then
        mp.osd_message(string.format("%d tabs (TAB for menu)", count), 2)
    end
end)
