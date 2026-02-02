.PHONY: build watch deploy

build:
	cargo run --release

watch:
	cargo run --release -- watch

deploy: build
	rsync -Pavzq ./dist/ megumu:/var/www/hauchiwa --delete
