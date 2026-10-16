;;; SPDX-FileCopyrightText: 2026 Mohamed Hammad <Mohamed.Hammad@SpacecraftSoftware.org>
;;; SPDX-License-Identifier: GPL-3.0-or-later
;;;
;;; Guix package for Fairing (Steelbore Standard §5.5): `guix build -f
;;; packaging/guix.scm`. It builds the release archive `cargo xtask dist`
;;; writes, whose crates are vendored and which tells cargo to use them, so
;;; cargo runs offline as the build container requires. `cargo xtask dist`
;;; writes the version and sha256; never edit them by hand.
;;;
;;; The vendored archive is built with cargo directly rather than with
;;; cargo-build-system, which expects every crate as a Guix package.

(use-modules (guix packages)
             (guix download)
             (guix gexp)
             (guix build-system gnu)
             ((guix licenses) #:prefix license:)
             (gnu packages rust)
             (gnu packages texinfo))

(package
  (name "fairing")
  (version "0.1.0")
  (source
   (origin
     (method url-fetch)
     (uri (string-append
           "https://github.com/Spacecraft-Software/Fairing/releases/download/v"
           version "/fairing-" version ".tar.gz"))
     (sha256 (base32 "19s47mfqvzcg4md5h5210l8dqzxv01sji0mv29qrmkmmgbx6am07"))))
  (build-system gnu-build-system)
  (arguments
   (list
    #:phases
    #~(modify-phases %standard-phases
        (delete 'configure)
        ;; cargo checks every vendored file against its recorded checksum;
        ;; rewriting their shebangs and Makefiles would fail that check, and
        ;; nothing the build runs needs them rewritten.
        (delete 'patch-source-shebangs)
        (delete 'patch-generated-file-shebangs)
        (add-before 'build 'set-cargo-home
          (lambda _
            (setenv "CARGO_HOME" (string-append (getcwd) "/.cargo-home"))))
        (replace 'build
          (lambda _
            (invoke "cargo" "build" "--frozen" "--release" "--package" "fairing")
            (invoke "make" "-C" "doc" "info")))
        (replace 'check
          (lambda* (#:key tests? #:allow-other-keys)
            (when tests?
              ;; xtask's tests read the repository itself, which the
              ;; archive leaves out.
              (invoke "cargo" "test" "--frozen" "--workspace"
                      "--exclude" "xtask"))))
        (replace 'install
          (lambda _
            (install-file "target/release/fairing"
                          (string-append #$output "/bin"))
            (install-file "doc/build/fairing.info"
                          (string-append #$output "/share/info")))))))
  (native-inputs (list rust `(,rust "cargo") texinfo))
  (home-page "https://Fairing.SpacecraftSoftware.org/")
  (synopsis "Boot splash for Steelbore OS Bravais")
  (description
   "Fairing draws boot progress on DRM/KMS from systemd's own job count,
answers LUKS passphrase questions inside the splash as a systemd password
agent, and hands the screen to greetd.")
  ;; The default build links LGPL-3.0-only malachite through the Nickel
  ;; evaluator, so it is conveyed under GPL-3.0 (DEPENDENCIES.md).
  (license (list license:gpl3 license:lgpl3)))
