-- Clear nominator data so it is rebuilt from scratch during reindex.
TRUNCATE indexer.nominators;

-- Reset the staking processor checkpoint so it reindexes from genesis.
-- This will reprocess all blocks with the corrected nominator logic
-- (owner registration, partial-withdrawal awareness, NominatedStakedUnlocked).
DELETE FROM indexer.metadata WHERE process = 'staking_processor_Consensus';
