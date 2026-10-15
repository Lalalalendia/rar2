//! Exact integer-EMU align/distribute planner.
//! Parity target: services/editor-api/align_distribute_plan_v1.py.
//! Pure geometry only: no editor mutation or authorization.
use std::collections::BTreeSet;

pub const MAX_SAFE_EMU: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectEmu {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub node_id: String,
    pub bounds: RectEmu,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedMember {
    pub node_id: String,
    pub before: RectEmu,
    pub after: RectEmu,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    AlignHorizontalCenter,
    AlignVerticalCenter,
    DistributeHorizontal,
    DistributeVertical,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanError {
    Empty,
    DuplicateNode,
    InvalidRect,
    Overflow,
    TooFewMembers,
}
fn safe(v: i128) -> Result<i64, PlanError> {
    if v < -(MAX_SAFE_EMU as i128) || v > MAX_SAFE_EMU as i128 {
        Err(PlanError::Overflow)
    } else {
        Ok(v as i64)
    }
}
fn validate(r: RectEmu) -> Result<(), PlanError> {
    for v in [r.x, r.y, r.width, r.height] {
        safe(v as i128)?;
    }
    if r.width <= 0 || r.height <= 0 {
        return Err(PlanError::InvalidRect);
    }
    safe(r.x as i128 + r.width as i128)?;
    safe(r.y as i128 + r.height as i128)?;
    Ok(())
}
fn round_half_away(n: i128) -> Result<i64, PlanError> {
    let sign = if n < 0 { -1 } else { 1 };
    safe(sign * ((n.abs() + 1) / 2))
}
pub fn plan(mut members: Vec<Member>, mode: Mode) -> Result<Vec<PlannedMember>, PlanError> {
    if members.is_empty() {
        return Err(PlanError::Empty);
    }
    let mut seen = BTreeSet::new();
    for m in &members {
        if m.node_id.is_empty() || !seen.insert(m.node_id.clone()) {
            return Err(PlanError::DuplicateNode);
        }
        validate(m.bounds)?;
    }
    members.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    let left = members.iter().map(|m| m.bounds.x).min().unwrap();
    let top = members.iter().map(|m| m.bounds.y).min().unwrap();
    let right = members
        .iter()
        .map(|m| m.bounds.x + m.bounds.width)
        .max()
        .unwrap();
    let bottom = members
        .iter()
        .map(|m| m.bounds.y + m.bounds.height)
        .max()
        .unwrap();
    let mut output: Vec<PlannedMember> = members
        .iter()
        .map(|m| PlannedMember {
            node_id: m.node_id.clone(),
            before: m.bounds,
            after: m.bounds,
        })
        .collect();
    match mode {
        Mode::DistributeHorizontal | Mode::DistributeVertical => {
            if members.len() < 3 {
                return Err(PlanError::TooFewMembers);
            }
            let horizontal = mode == Mode::DistributeHorizontal;
            let mut order: Vec<usize> = (0..members.len()).collect();
            order.sort_by(|&i, &j| {
                let a = members[i].bounds;
                let b = members[j].bounds;
                let ka = if horizontal {
                    (a.x, a.x + a.width)
                } else {
                    (a.y, a.y + a.height)
                };
                let kb = if horizontal {
                    (b.x, b.x + b.width)
                } else {
                    (b.y, b.y + b.height)
                };
                ka.cmp(&kb)
                    .then_with(|| members[i].node_id.cmp(&members[j].node_id))
            });
            let first = members[order[0]].bounds;
            let last = members[*order.last().unwrap()].bounds;
            let start = if horizontal { first.x } else { first.y };
            let end = if horizontal {
                last.x + last.width
            } else {
                last.y + last.height
            };
            let total: i128 = members
                .iter()
                .map(|m| {
                    if horizontal {
                        m.bounds.width as i128
                    } else {
                        m.bounds.height as i128
                    }
                })
                .sum();
            safe(total)?;
            let total_gap = safe(end as i128 - start as i128 - total)?;
            let count = (order.len() - 1) as i64;
            let q = total_gap.div_euclid(count);
            let rem = total_gap.rem_euclid(count);
            let mut cursor = if horizontal {
                first.x + first.width
            } else {
                first.y + first.height
            };
            for (index, &item) in order.iter().enumerate().skip(1) {
                let gap = q + if (index as i64 - 1) < rem { 1 } else { 0 };
                let desired = safe(cursor as i128 + gap as i128)?;
                let r = members[item].bounds;
                if index == order.len() - 1 {
                    let actual = if horizontal { r.x } else { r.y };
                    if actual != desired {
                        return Err(PlanError::Overflow);
                    }
                } else if horizontal {
                    output[item].after.x = desired;
                } else {
                    output[item].after.y = desired;
                }
                let after = output[item].after;
                validate(after)?;
                cursor = if horizontal {
                    after.x + after.width
                } else {
                    after.y + after.height
                };
            }
        }
        _ => {
            for item in &mut output {
                let r = item.before;
                match mode {
                    Mode::AlignLeft => item.after.x = left,
                    Mode::AlignRight => item.after.x = safe(right as i128 - r.width as i128)?,
                    Mode::AlignTop => item.after.y = top,
                    Mode::AlignBottom => item.after.y = safe(bottom as i128 - r.height as i128)?,
                    Mode::AlignHorizontalCenter => {
                        item.after.x =
                            round_half_away(left as i128 + right as i128 - r.width as i128)?
                    }
                    Mode::AlignVerticalCenter => {
                        item.after.y =
                            round_half_away(top as i128 + bottom as i128 - r.height as i128)?
                    }
                    _ => unreachable!(),
                }
                validate(item.after)?;
            }
        }
    }
    Ok(output)
}
/// Convert only changed members into a batch. The caller must validate all selected
/// members before calling this helper; unchanged outer anchors remain selected.
pub fn changed_only(plan: Vec<PlannedMember>) -> Vec<PlannedMember> {
    plan.into_iter().filter(|m| m.before != m.after).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn m(id: &str, x: i64, w: i64) -> Member {
        Member {
            node_id: id.into(),
            bounds: RectEmu {
                x,
                y: 0,
                width: w,
                height: 10,
            },
        }
    }
    #[test]
    fn signed_gaps_and_unchanged_anchors() {
        let result = plan(
            vec![m("c", 7, 10), m("a", 0, 10), m("b", 4, 10)],
            Mode::DistributeHorizontal,
        )
        .unwrap();
        assert_eq!(result.iter().find(|v| v.node_id == "b").unwrap().after.x, 3);
        assert_eq!(result.iter().find(|v| v.node_id == "a").unwrap().after.x, 0);
        assert_eq!(result.iter().find(|v| v.node_id == "c").unwrap().after.x, 7);
        assert_eq!(changed_only(result).len(), 1);
    }
    #[test]
    fn euclidean_negative_remainder_and_fixed_outer_members() {
        // X starts: 0, 2, 4, 5; widths: 10 each.
        // Occupied span=15, total widths=40, total gap=-25.
        // Euclidean gaps are [-8, -8, -9], preserving the first and last.
        let result = plan(
            vec![m("d", 5, 10), m("c", 4, 10), m("a", 0, 10), m("b", 2, 10)],
            Mode::DistributeHorizontal,
        )
        .unwrap();
        assert_eq!(result[0].after.x, 0);
        assert_eq!(result[1].after.x, 2);
        assert_eq!(result[2].after.x, 4);
        assert_eq!(result[3].after.x, 5);
        assert!(changed_only(result).is_empty());
    }

    #[test]
    fn half_away_from_zero() {
        assert_eq!(round_half_away(-5).unwrap(), -3);
        assert_eq!(round_half_away(5).unwrap(), 3);
    }
    #[test]
    fn reject_duplicates_and_overflow() {
        assert_eq!(
            plan(vec![m("a", 0, 1), m("a", 2, 1)], Mode::AlignLeft),
            Err(PlanError::DuplicateNode)
        );
        assert_eq!(
            plan(vec![m("a", MAX_SAFE_EMU, 2)], Mode::AlignLeft),
            Err(PlanError::Overflow)
        );
    }
}
