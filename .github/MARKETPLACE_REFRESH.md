# Marketplace refresh after a release

After publishing the plugin archives, the release workflow requests a rebuild
of the Swack Tools marketplace. The separate notification job requires the
Actions secret `MARKETPLACE_DISPATCH_TOKEN` with **Actions: write** permission
on `swack-tools/ai-plugin-marketplace`.

Follow the marketplace's [authentication and rollout instructions](https://github.com/swack-tools/ai-plugin-marketplace/blob/main/docs/maintaining-catalog.md#release-notifications)
before merging this change. If notification fails, the release remains published.
Fix the error and rerun only failed jobs to retry the notification. Confirm
publication in the marketplace's Actions run; acceptance of the request does
not confirm deployment.
