# mastodon-block-enum
A small tool to analyze blocked domains on mastodon instances

## Usage
1. Build using [cargo](https://rustup.rs/)
2. Create the initial database using `mastodon-block-enum fetch`
3. Brute-force some of censored domains using `mastodon-block-enum crack` until it starts taking too long or you get bored
4. Show a list of all blocked domains using `mastodon-block-enum show`
