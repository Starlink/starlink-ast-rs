# Copyright 2026 Association of Universities for Research in Astronomy (AURA)
# SPDX-License-Identifier: LGPL-3.0-or-later

import merge_card_hunks as m

OLD = ("NIN_A   =                    2 / Number of input coordinates\n"
       "SFT2_A  =      -223.8380193875 / Shift for axis 2\n"
       "END\n")
NEW = ("NIN_A   =                    2 / Number of input coordinates\n"
       "ISSIMP_B=                    1 / Mapping has been simplified\n"
       "SFT2_A  =  -223.83801938750003 / Shift for axis 2\n"
       "END\n")


def test_takes_cards_from_new_and_values_from_old():
    merged = m.merge(OLD, NEW)
    assert "ISSIMP_B" in merged
    assert "-223.8380193875 " in merged
    assert "-223.83801938750003" not in merged
    m.check(OLD, NEW, merged)


def test_check_rejects_a_non_separable_pair():
    bad = NEW.replace("NIN_A   =                    2", "NIN_A   =                    3")
    try:
        m.check(OLD, bad, m.merge(OLD, bad))
    except ValueError:
        return
    raise AssertionError("expected ValueError for a non-separable pair")


def test_check_allows_a_last_bits_float_difference():
    m.assert_separable(OLD, NEW)


def test_check_rejects_a_changed_comment():
    bad = NEW.replace("/ Shift for axis 2", "/ Shift for axis 3")
    try:
        m.check(OLD, bad, m.merge(OLD, bad))
    except ValueError:
        return
    raise AssertionError("expected ValueError for a changed comment")


def test_check_rejects_a_dropped_card():
    bad = NEW.replace("SFT2_A  =  -223.83801938750003 / Shift for axis 2\n", "")
    try:
        m.check(OLD, bad, m.merge(OLD, bad))
    except ValueError:
        return
    raise AssertionError("expected ValueError for a dropped card")
