// @generated automatically by Diesel CLI.

diesel::table! {
    intervals (id) {
        id -> BigInt,
        serial -> Text,
        started -> BigInt,
        ended -> BigInt,
        seconds -> Double,
        first -> Double,
        last -> Double,
        charging -> Bool,
        poll_rate -> Integer,
        vid -> Integer,
        pid -> Integer,
        session -> BigInt,
    }
}

diesel::table! {
    samples (id) {
        id -> BigInt,
        serial -> Text,
        timestamp -> BigInt,
        session -> BigInt,
        battery -> Double,
        charging -> Bool,
        poll_rate -> Integer,
        vid -> Integer,
        pid -> Integer,
    }
}

diesel::allow_tables_to_appear_in_same_query!(intervals, samples,);
