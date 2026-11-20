-- The session engine was called "flow" in the schema. It is the engine: a
-- session either is run by it or not, and what it was given is its input.
alter table sessions add column engine boolean not null default false;
update sessions set engine = true where flow = 'engine';
alter table sessions drop column flow;
alter table sessions rename column flow_start to engine_start;

alter table session_flow_inputs rename to session_engine_inputs;
alter index session_flow_inputs_pkey rename to session_engine_inputs_pkey;
alter table session_engine_inputs rename constraint session_flow_inputs_session_id_fkey to session_engine_inputs_session_id_fkey;
alter table session_engine_inputs rename constraint session_flow_inputs_at_ms_check to session_engine_inputs_at_ms_check;
alter table session_engine_inputs rename constraint session_flow_inputs_seq_check to session_engine_inputs_seq_check;
