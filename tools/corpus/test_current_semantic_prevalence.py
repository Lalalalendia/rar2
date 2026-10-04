#!/usr/bin/env python3
from current_semantic_prevalence import aggregate

rows=[
 {"opened":True,"format_version":"0x2c","semantic_prevalence":{
   "image_crop":{"state":"observed_viewer_projection","placement_count":2},
   "mature_page_roles":{"state":"observed_current_reader_authority","group_child_count":3,"applied_master_relation_count":1},
 }},
 {"opened":True,"format_version":"0x22","semantic_prevalence":{
   "image_crop":{"state":"observed_viewer_projection","placement_count":0},
   "mature_page_roles":{"state":"not_applicable_non_mature_0x2c"},
 }},
 {"opened":False},
]
r=aggregate(rows)
assert r["corpus_file_count"]==3,r
assert r["opened_file_count"]==2,r
assert r["image_crop"]["files_with_observed_crop"]==1,r
assert r["image_crop"]["observed_crop_placement_count"]==2,r
assert r["mature_group_children"]["files_with_group_children"]==1,r
assert r["mature_group_children"]["group_child_count"]==3,r
assert r["applied_master_relations"]["files_with_applied_master_relation"]==1,r
assert r["applied_master_relations"]["applied_master_relation_count"]==1,r
assert r["grounded_guides"]["count"] is None,r
print("ok")
