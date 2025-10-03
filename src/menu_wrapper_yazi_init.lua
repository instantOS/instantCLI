local function current_name()
    local h = cx.active and cx.active.current and cx.active.current.hovered
    if h == nil then
        return ""
    end

    if h.link_to and h.link_to ~= "" then
        return string.format("%s → %s", h.name or "", h.link_to)
    end

    return h.name or ""
end

local function hint_segment()
    local hint = os.getenv("INS_MENU_PICKER_HINT")
    if hint == nil or hint == "" then
        if os.getenv("INS_MENU_PICKER_MULTI") == "1" then
            hint = "Tab to mark, Enter to confirm"
        else
            hint = "Enter to choose, Esc to cancel"
        end
    end

    local scope = os.getenv("INS_MENU_PICKER_SCOPE")
    if scope == "files" then
        hint = hint .. " • files only"
    elseif scope == "directories" then
        hint = hint .. " • directories only"
    end

    return ui.Line {
        ui.Span(hint):fg("green"):bold(),
    }
end

Status = {}

function Status:render(area)
    local name_line = ui.Line { ui.Span(current_name()):fg("blue") }
    local right = hint_segment()

    return {
        ui.Bar(area, ui.Bar.BOTTOM):symbol(" "),
        ui.Paragraph(area, { name_line }):align(ui.Paragraph.CENTER),
        ui.Paragraph(area, { right }):align(ui.Paragraph.RIGHT),
    }
end
