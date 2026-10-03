---
expect:
  operation_id: [postSyncRatingsAdd]
  confirmed: true
  body.movies: array
---

{"operation_id":"postSyncRatingsAdd","status":201,"data":{"added":{"movies":1,"shows":0,"seasons":0,"episodes":0},"not_found":{"movies":[],"shows":[],"seasons":[],"episodes":[]}},"pagination":null}
