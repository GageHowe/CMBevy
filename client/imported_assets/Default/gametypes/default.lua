-- FFA deathmatch

RESPAWN_DELAY = 5.0
TEAMS_ENABLED = false
SCORING = "score_to_win"
LEADERBOARD_SCOPE = "player"
LEADERBOARD_NUMBER_INDEX = 0
SCORE_TO_WIN = 50
TIME_LIMIT_SECS = 600.0
TEAM_COUNT = 2
LEADERBOARD_LABEL = "Kills"
PRIMARY_OBJECTIVE_LABEL = "Eliminate enemies"

-- function on_tick()
--     if IS_SERVER then
-- --        print("on_tick running on the server.")
--     else
-- --        print("on_tick running on the client.")
--     end
-- end
--
-- function on_fixed_tick()
--     if not IS_SERVER then
-- --        print("Fixed tick running on client.")
--     else
-- --        print("Fixed tick running on server.")
--     end
-- end
--
-- print("Starting match!")
--
-- if IS_SERVER then
--     print("Starting as server!")
-- else
--     print("Starting as client!")
-- end
