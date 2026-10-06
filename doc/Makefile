# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Fairing manual build (Steelbore Standard §8). POSIX make; run from doc/ or via
# the root Makefile. `check` is the zero-warning gate CI runs.
NAME  = fairing
SRC   = $(NAME).texi
CSS   = spacecraft.css
BUILD = build

all: info html text

info: $(BUILD)
	makeinfo --no-split $(SRC) -o $(BUILD)/$(NAME).info

html: $(BUILD)
	texi2any --html --no-split --css-include=$(CSS) $(SRC) -o $(BUILD)/$(NAME).html

text: $(BUILD)
	texi2any --plaintext --no-split $(SRC) -o $(BUILD)/$(NAME).txt

pdf: $(BUILD)
	texi2pdf --texinfo=@afourpaper --quiet --clean -o $(BUILD)/$(NAME).pdf $(SRC)

check:
	makeinfo --no-split $(SRC) -o /dev/null

requirements:
	cd .. && cargo xtask req-texi

$(BUILD):
	mkdir -p $(BUILD)

clean:
	rm -rf $(BUILD) *.t2d *.aux *.toc *.log

.PHONY: all info html text pdf check requirements clean
