# SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Steelbore Standard §8.2: the top-level Makefile exposes info, html and pdf.
# Everything delegates to doc/Makefile; the Rust build is `cargo build`.
info html text pdf check requirements clean:
	$(MAKE) -C doc $@

.PHONY: info html text pdf check requirements clean
