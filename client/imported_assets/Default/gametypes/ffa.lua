-- Free-for-all slayer driven by player kill callbacks.

PLAYER_NUMBER = {
    kills = 0,
}

RESPAWN_DELAY = 5.0
TEAMS_ENABLED = false
SCORING = "score_to_win"
LEADERBOARD_SCOPE = "player"
LEADERBOARD_NUMBER_INDEX = PLAYER_NUMBER.kills
SCORE_TO_WIN = 25
TIME_LIMIT_SECS = 600.0
TEAM_COUNT = 2
LEADERBOARD_LABEL = "Kills"

POST_GAME_DELAY_SECS = 3.0
BOT_SPAWN_SECS = 5.0

local next_bot_spawn = 0.0

function spawn_hostile_bot_on_team(team)
    local bot = spawn_bot(team, "killer")
    if bot ~= nil then
        give_weapon(bot, "thumper")
        show_message("spawned bot")
        return true
    end
    return false
end

function on_fixed_tick()
    if not IS_SERVER then
        return
    end

    if get_match_phase() == "post_game" and get_match_phase_time() >= POST_GAME_DELAY_SECS then
        restart_round()
        next_bot_spawn = BOT_SPAWN_SECS
        return
    end

    if get_match_phase() ~= "playing" then
        return
    end

    local t = get_match_phase_time()
    if t >= next_bot_spawn then
        next_bot_spawn = t + BOT_SPAWN_SECS
        spawn_hostile_bot_on_team(0)
        spawn_hostile_bot_on_team(1)
    end
end

function on_player_killed(victim, killer)
    if not IS_SERVER then
        return
    end
    if is_bot(victim) then
        local respawn_at = get_match_phase_time() + RESPAWN_DELAY
        if respawn_at < next_bot_spawn then
            next_bot_spawn = respawn_at
        end
        show_message("bot died")
    end
    if killer == nil or killer == victim then
        -- show_message("A player died")
        return
    end

    add_player_number(killer, PLAYER_NUMBER.kills, 1)
    if get_player_number(killer, PLAYER_NUMBER.kills) >= SCORE_TO_WIN then
        show_message("A player won the match")
        end_game_with_player_winner(killer)
    end
end
