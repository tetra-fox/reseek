-- sample transfer events written before every transfer had a state hold "None", which no
-- transfer is in now; they read as the state a sample transfer is in
UPDATE Events SET Data = json_set(Data, '$.transfer.state', 'Queued, Locally')
WHERE json_valid(Data) AND json_extract(Data, '$.transfer.state') = 'None';
