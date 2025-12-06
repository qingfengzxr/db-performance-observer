CREATE DATABASE IF NOT EXISTS perf
  CHARACTER SET utf8mb4
  COLLATE utf8mb4_0900_ai_ci;

USE perf;

DROP TABLE IF EXISTS events;

CREATE TABLE events (
  id BIGINT PRIMARY KEY AUTO_INCREMENT,
  user_id BIGINT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  amount DECIMAL(10,2) NOT NULL,
  status SMALLINT NOT NULL,
  category INT NOT NULL,
  payload VARCHAR(200) NOT NULL,
  INDEX idx_user_created (user_id, created_at),
  INDEX idx_status (status),
  INDEX idx_created_at (created_at)
) ENGINE=InnoDB;
