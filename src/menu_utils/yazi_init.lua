local function build_hint()
    local hint = os.getenv("INS_MENU_PICKER_HINT") or ""
    local is_multi = os.getenv("INS_MENU_PICKER_MULTI") == "1"

    if hint == "" then
        if is_multi then
            hint = "Enter: confirm • Tab: toggle • Esc: cancel"
        else
            hint = "Enter: choose • Esc: cancel"
        end
    end

    local scope = os.getenv("INS_MENU_PICKER_SCOPE")
    if scope == "files" then
        hint = hint .. " • selecting files"
    elseif scope == "directories" then
        hint = hint .. " • selecting directories"
    end

    return hint
end

local picker_hint = build_hint()

Status:children_add(function()
    return ui.Line {
        ui.Span(" menu picker "):bg("green"):fg("black"):bold(),
        ui.Span(" "),
        ui.Span(picker_hint):fg("green"),
    }
end, 100, Status.LEFT)
