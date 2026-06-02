-- King of the Hill
--
-- Map setup:
-- Add ScriptZone + ScriptTags { tags: ["hill"] } to the hill volume entity.
-- Any player inside that zone gains score over time.

PLAYER_NUMBER = {
    score = 0,
}

RESPAWN_DELAY = 5.0
TEAMS_ENABLED = false
SCORING = "score_to_win"
LEADERBOARD_SCOPE = "player"
LEADERBOARD_NUMBER_INDEX = PLAYER_NUMBER.score
SCORE_TO_WIN = 100
TIME_LIMIT_SECS = 600.0
TEAM_COUNT = 2
LEADERBOARD_LABEL = "Hill Score"
PRIMARY_OBJECTIVE_LABEL = "Hold the hill"

KOTH_SCORE_INTERVAL_SECS = 1.0
POST_GAME_DELAY_SECS = 3.0

local score_timer = 0.0

function on_fixed_tick()
    if not IS_SERVER then
        return
    end

    if get_match_phase() == "post_game" then
        if get_match_phase_time() >= POST_GAME_DELAY_SECS then
            restart_round()
        end
        return
    end

    score_timer = score_timer + FIXED_DELTA_SECONDS
    if score_timer < KOTH_SCORE_INTERVAL_SECS then
        return
    end
    score_timer = 0.0

    local hill = get_first_tagged("hill")
    if hill == nil then
        return
    end

    for _, entity in ipairs(get_entities_in_zone(hill)) do
        if is_player(entity) then
            add_player_number(entity, PLAYER_NUMBER.score, 1)
            if get_player_number(entity, PLAYER_NUMBER.score) >= SCORE_TO_WIN then
                end_game_with_player_winner(entity)
                return
            end
        end
    end
end
