CREATE TABLE samples (
    id BIGINT PRIMARY KEY NOT NULL,
    serial TEXT NOT NULL,
    timestamp BIGINT NOT NULL CHECK (timestamp > 0),
    session BIGINT NOT NULL,
    battery DOUBLE NOT NULL CHECK (battery BETWEEN 0 AND 100),
    charging BOOLEAN NOT NULL,
    poll_rate INTEGER NOT NULL CHECK (poll_rate BETWEEN 0 AND 65535),
    vid INTEGER NOT NULL CHECK (vid BETWEEN 0 AND 65535),
    pid INTEGER NOT NULL CHECK (pid BETWEEN 0 AND 65535)
);
CREATE INDEX sample_device_time ON samples (serial, timestamp, id);
CREATE INDEX sample_age ON samples (timestamp);

CREATE TABLE intervals (
    id BIGINT PRIMARY KEY NOT NULL,
    serial TEXT NOT NULL,
    started BIGINT NOT NULL,
    ended BIGINT NOT NULL,
    seconds DOUBLE NOT NULL CHECK (seconds > 0),
    first DOUBLE NOT NULL CHECK (first BETWEEN 0 AND 100),
    last DOUBLE NOT NULL CHECK (last BETWEEN 0 AND 100),
    charging BOOLEAN NOT NULL,
    poll_rate INTEGER NOT NULL CHECK (poll_rate BETWEEN 0 AND 65535),
    vid INTEGER NOT NULL CHECK (vid BETWEEN 0 AND 65535),
    pid INTEGER NOT NULL CHECK (pid BETWEEN 0 AND 65535),
    session BIGINT NOT NULL
);
CREATE INDEX interval_device ON intervals (serial, charging, vid, pid, ended DESC);
CREATE INDEX interval_age ON intervals (ended);
