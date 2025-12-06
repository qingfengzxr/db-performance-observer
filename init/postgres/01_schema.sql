-- Schema for PostgreSQL runs
DROP TABLE IF EXISTS public.events;

CREATE TABLE public.events (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  amount NUMERIC(10,2) NOT NULL,
  status SMALLINT NOT NULL,
  category INT NOT NULL,
  payload VARCHAR(200) NOT NULL
);

CREATE INDEX idx_user_created ON public.events (user_id, created_at);
CREATE INDEX idx_status ON public.events (status);
CREATE INDEX idx_created_at ON public.events (created_at);
