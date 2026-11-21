package project

import (
	"bufio"
	"bytes"
	"encoding/json"
	"io"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/evanw/esbuild/pkg/api"
	"github.com/paralin/modlock/cli"
)

// CheckError is a build that failed on problems in the sources. Diagnostics
// locates each problem the checks and compilers reported.
type CheckError struct {
	// Err describes the failure in one line.
	Err error
	// Diagnostics lists the problems.
	Diagnostics []*cli.Diagnostic
}

// Error returns the one-line description.
func (e *CheckError) Error() string {
	return e.Err.Error()
}

// Unwrap returns the one-line description's error.
func (e *CheckError) Unwrap() error {
	return e.Err
}

// RustTarget is the target a Rust mod compiles for.
const RustTarget = "wasm32-wasip1"

// rustMissing says how to get what a Rust mod builds with.
const rustMissing = "a Rust mod needs Rust: install it from https://rustup.rs/, run rustup target add " + RustTarget + ", open a new terminal and run this again"

// cargoMessage is one line of cargo build --message-format json: a file
// Cargo built, or a problem the compiler reported.
type cargoMessage struct {
	Reason    string   `json:"reason"`
	Filenames []string `json:"filenames"`
	Message   *struct {
		Level    string `json:"level"`
		Message  string `json:"message"`
		Rendered string `json:"rendered"`
		Code     *struct {
			Code string `json:"code"`
		} `json:"code"`
		Spans []struct {
			File    string `json:"file_name"`
			Line    uint32 `json:"line_start"`
			Column  uint32 `json:"column_start"`
			Primary bool   `json:"is_primary"`
		} `json:"spans"`
	} `json:"message"`
}

// cargoMessages reads the output of cargo build --message-format json, run
// in dir. It writes each problem to output as the compiler rendered it, and
// returns the WebAssembly module Cargo built, if any, and where each error
// and warning is.
func cargoMessages(dir string, reported []byte, output io.Writer) (string, []*cli.Diagnostic) {
	built := ""
	var found []*cli.Diagnostic
	for line := range bytes.Lines(reported) {
		var message cargoMessage
		if json.Unmarshal(line, &message) != nil {
			continue
		}
		for _, name := range message.Filenames {
			if message.Reason == "compiler-artifact" && filepath.Ext(name) == ".wasm" {
				built = name
			}
		}
		problem := message.Message
		if message.Reason != "compiler-message" || problem == nil {
			continue
		}
		_, _ = io.WriteString(output, problem.Rendered)
		if problem.Level != "error" && problem.Level != "warning" {
			continue
		}

		// A problem is at its primary span; one without is the build's
		// summary, which locates nothing.
		for _, span := range problem.Spans {
			if !span.Primary {
				continue
			}
			diagnostic := &cli.Diagnostic{
				File:    absolute(dir, span.File),
				Line:    span.Line,
				Column:  span.Column,
				Message: problem.Message,
				Source:  "cargo build",
			}
			if problem.Code != nil {
				diagnostic.Code = problem.Code.Code
			}
			found = append(found, diagnostic)
			break
		}
	}
	return built, found
}

// tsgoLine matches the first line of a tsgo diagnostic printed with
// --pretty false: path(line,column): error TS1234: message.
var tsgoLine = regexp.MustCompile(`^(.+)\((\d+),(\d+)\): (?:error|warning) (TS\d+): (.*)$`)

// goLine matches a go vet or go build diagnostic: path.go:line:column: message,
// with the column optional and vet's own prefix allowed.
var goLine = regexp.MustCompile(`^(?:vet: )?(.+\.go):(\d+)(?::(\d+))?: (.*)$`)

// luauLine matches a luau-analyze diagnostic: path(line,column): Kind: message.
var luauLine = regexp.MustCompile(`^(.+)\((\d+),(\d+)\): (\w+): (.*)$`)

// tsgoDiagnostics reads tsgo's output from a check of the project in dir.
// Indented lines continue the diagnostic above them.
func tsgoDiagnostics(dir string, output []byte) []*cli.Diagnostic {
	var found []*cli.Diagnostic
	scanner := bufio.NewScanner(bytes.NewReader(output))
	for scanner.Scan() {
		line := scanner.Text()
		if match := tsgoLine.FindStringSubmatch(line); match != nil {
			found = append(found, &cli.Diagnostic{
				File:    absolute(dir, match[1]),
				Line:    number(match[2]),
				Column:  number(match[3]),
				Message: match[5],
				Source:  "tsgo",
				Code:    match[4],
			})
			continue
		}
		if last := len(found) - 1; last >= 0 && strings.HasPrefix(line, " ") {
			found[last].Message += "\n" + strings.TrimSpace(line)
		}
	}
	return found
}

// goDiagnostics reads the output of go vet or go build, run as source in dir.
func goDiagnostics(dir, source string, output []byte) []*cli.Diagnostic {
	var found []*cli.Diagnostic
	scanner := bufio.NewScanner(bytes.NewReader(output))
	for scanner.Scan() {
		match := goLine.FindStringSubmatch(scanner.Text())
		if match == nil {
			continue
		}
		found = append(found, &cli.Diagnostic{
			File:    absolute(dir, match[1]),
			Line:    number(match[2]),
			Column:  number(match[3]),
			Message: match[4],
			Source:  source,
		})
	}
	return found
}

// luauDiagnostics reads luau-analyze's output from a check of the project in
// dir.
func luauDiagnostics(dir string, output []byte) []*cli.Diagnostic {
	var found []*cli.Diagnostic
	scanner := bufio.NewScanner(bytes.NewReader(output))
	for scanner.Scan() {
		match := luauLine.FindStringSubmatch(scanner.Text())
		if match == nil {
			continue
		}
		found = append(found, &cli.Diagnostic{
			File:    absolute(dir, match[1]),
			Line:    number(match[2]),
			Column:  number(match[3]),
			Message: match[5],
			Source:  "luau-analyze",
			Code:    match[4],
		})
	}
	return found
}

// esbuildDiagnostics converts esbuild's messages for a build in dir.
func esbuildDiagnostics(dir string, messages []api.Message) []*cli.Diagnostic {
	found := make([]*cli.Diagnostic, 0, len(messages))
	for _, message := range messages {
		diagnostic := &cli.Diagnostic{Message: message.Text, Source: "esbuild"}
		if location := message.Location; location != nil {
			diagnostic.File = absolute(dir, location.File)
			diagnostic.Line = uint32(location.Line)
			diagnostic.Column = uint32(location.Column) + 1
		}
		found = append(found, diagnostic)
	}
	return found
}

// absolute resolves a tool's path against the directory it ran in.
func absolute(dir, path string) string {
	if filepath.IsAbs(path) {
		return filepath.Clean(path)
	}
	return filepath.Join(dir, path)
}

// number parses a line or column, zero when absent.
func number(text string) uint32 {
	value, _ := strconv.ParseUint(text, 10, 32)
	return uint32(value)
}
