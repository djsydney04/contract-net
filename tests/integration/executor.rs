use crate::fixtures::reference_cases;
use contractnet::{benchmark, run_task};
use serde_json::json;

#[test]
fn all_python_reference_answers_match() {
    for case in reference_cases() {
        let actual = run_task(&case.task_type, &case.params).unwrap();
        assert_eq!(
            actual.to_string(),
            case.expected,
            "{} {}",
            case.task_type,
            case.params
        );
    }
}

#[test]
fn invalid_tasks_fail_without_hanging() {
    for (kind, params) in [
        ("hash_search", json!({"seed":0,"threshold":0})),
        ("hash_search", json!({"seed":0,"threshold":-1})),
        ("matmul_mod", json!({"seed":0,"n":2,"mod":0})),
        ("matmul_mod", json!({"seed":0,"n":2,"mod":-3})),
        ("sort_checksum", json!({"seed":0})),
        (
            "sort_checksum",
            json!({"seed":0,"n":"18446744073709551616"}),
        ),
        ("monte_carlo_pi", json!({"seed":"nonsense","samples":10})),
        ("unknown", json!({})),
    ] {
        assert!(run_task(kind, &params).is_err(), "{kind} {params}");
    }
}

#[test]
fn empty_tasks_and_numeric_strings_match_python() {
    assert_eq!(
        run_task("sort_checksum", &json!({"seed":" -99 ","n":"0"}))
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        run_task("monte_carlo_pi", &json!({"seed":0,"samples":-1}))
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        run_task("matmul_mod", &json!({"seed":1,"n":-2,"mod":97}))
            .unwrap()
            .to_string(),
        "0"
    );
}

#[test]
fn work_model_tracks_the_optimized_matrix_algorithm() {
    let small = benchmark::work_units("matmul_mod", &json!({"n":40,"mod":1000003})).unwrap();
    let large = benchmark::work_units("matmul_mod", &json!({"n":80,"mod":1000003})).unwrap();
    assert_eq!(large / small, 4.0);
    assert!(benchmark::work_units("hash_search", &json!({"threshold":0})).is_err());
}
