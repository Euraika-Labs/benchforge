from app import login


def test_successful_login():
    res = login("alice", "correct-horse-battery-staple", "10.0.0.1")
    assert res.status_code == 200


def test_three_failures_allowed_then_rate_limited():
    ip = "10.0.0.2"
    assert login("alice", "bad", ip).status_code == 401
    assert login("alice", "bad", ip).status_code == 401
    assert login("alice", "bad", ip).status_code == 401
    assert login("alice", "bad", ip).status_code == 429


def test_success_resets_failures():
    ip = "10.0.0.3"
    assert login("alice", "bad", ip).status_code == 401
    assert login("alice", "correct-horse-battery-staple", ip).status_code == 200
    assert login("alice", "bad", ip).status_code == 401
