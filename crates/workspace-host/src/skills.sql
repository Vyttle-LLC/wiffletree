-- The skill merges whose note the human dismissed on this machine, by skill folder and the
-- "Merged" history entry's name.
CREATE TABLE IF NOT EXISTS skill_merges_dismissed (skill TEXT NOT NULL, entry TEXT NOT NULL, PRIMARY KEY (skill, entry));
