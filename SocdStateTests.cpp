#include "SocdState.h"

#include <array>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace {

using lastkey::EventDisposition;
using lastkey::Key;
using lastkey::KeyAction;
using lastkey::SocdState;

enum class AttemptResult { Failure, Success };

constexpr AttemptResult kFailure = AttemptResult::Failure;
constexpr AttemptResult kSuccess = AttemptResult::Success;

struct OutputAttempt {
    Key key;
    KeyAction action;
    bool operator==(const OutputAttempt& other) const {
        return key == other.key && action == other.action;
    }
};

struct TestEmitter {
    bool operator()(Key key, KeyAction action) {
        attempts.push_back({key, action});
        if (nextResult >= attemptResults.size())
            throw std::runtime_error("unexpected output attempt");
        return attemptResults[nextResult++] == AttemptResult::Success;
    }

    void Prepare(std::vector<AttemptResult> nextAttemptResults) {
        attemptResults = std::move(nextAttemptResults);
        nextResult = 0;
        attempts.clear();
    }

    std::vector<AttemptResult> attemptResults;
    std::size_t nextResult = 0;
    std::vector<OutputAttempt> attempts;
};

using OutputState = std::array<bool, lastkey::kKeyCount>;

OutputState NoOutput() { return {}; }

OutputState OutputFor(Key key) {
    OutputState output{};
    output[lastkey::ToIndex(key)] = true;
    return output;
}

OutputState OutputFor(Key first, Key second) {
    OutputState output = OutputFor(first);
    output[lastkey::ToIndex(second)] = true;
    return output;
}

struct Step {
    Key key;
    KeyAction action;
    std::vector<AttemptResult> attemptResults;
    EventDisposition disposition;
    std::vector<OutputAttempt> attempts;
    OutputState output;
};

std::string_view KeyName(Key key) {
    switch (key) {
    case Key::VerticalFirst: return "VerticalFirst";
    case Key::VerticalSecond: return "VerticalSecond";
    case Key::HorizontalFirst: return "HorizontalFirst";
    case Key::HorizontalSecond: return "HorizontalSecond";
    case Key::Count: return "Count";
    }
    return "Unknown";
}

std::string_view ActionName(KeyAction action) {
    return action == KeyAction::Down ? "Down" : "Up";
}

std::string_view DispositionName(EventDisposition disposition) {
    return disposition == EventDisposition::Consume ? "Consume" : "PassThrough";
}

std::string DescribeAttempts(const std::vector<OutputAttempt>& attempts) {
    std::string description = "[";
    for (std::size_t index = 0; index < attempts.size(); ++index) {
        if (index != 0) description += ", ";
        description += KeyName(attempts[index].key);
        description += ' ';
        description += ActionName(attempts[index].action);
    }
    return description + ']';
}

std::string StepPrefix(std::string_view testName, std::size_t stepIndex, const Step& step) {
    return std::string(testName) + ", step " + std::to_string(stepIndex + 1) + " (" +
           std::string(KeyName(step.key)) + ' ' + std::string(ActionName(step.action)) + "): ";
}

void Require(bool condition, const std::string& message) {
    if (!condition) throw std::runtime_error(message);
}

void RequireNoOpposingOutputs(const SocdState& filter, const std::string& prefix) {
    Require(!(filter.OutputHeld(Key::VerticalFirst) && filter.OutputHeld(Key::VerticalSecond)),
            prefix + "vertical opposing outputs are both held");
    Require(!(filter.OutputHeld(Key::HorizontalFirst) && filter.OutputHeld(Key::HorizontalSecond)),
            prefix + "horizontal opposing outputs are both held");
}

void RunSteps(std::string_view name, const std::vector<Step>& steps) {
    SocdState filter;
    TestEmitter emitter;

    for (std::size_t stepIndex = 0; stepIndex < steps.size(); ++stepIndex) {
        const Step& step = steps[stepIndex];
        const std::string prefix = StepPrefix(name, stepIndex, step);
        emitter.Prepare(step.attemptResults);
        const EventDisposition actual = filter.Process(step.key, step.action, emitter);
        Require(actual == step.disposition,
                prefix + "expected " + std::string(DispositionName(step.disposition)) +
                    ", got " + std::string(DispositionName(actual)));
        Require(emitter.attempts == step.attempts,
                prefix + "expected output attempts " + DescribeAttempts(step.attempts) +
                    ", got " + DescribeAttempts(emitter.attempts));
        Require(emitter.nextResult == emitter.attemptResults.size(),
                prefix + "used " + std::to_string(emitter.nextResult) + " of " +
                    std::to_string(emitter.attemptResults.size()) + " configured attempt results");
        for (std::size_t index = 0; index < lastkey::kKeyCount; ++index) {
            const bool actualOutput = filter.OutputHeld(static_cast<Key>(index));
            Require(actualOutput == step.output[index],
                    prefix + "expected outputHeld(" +
                        std::string(KeyName(static_cast<Key>(index))) + ")=" +
                        (step.output[index] ? "true" : "false") + ", got " +
                        (actualOutput ? "true" : "false"));
        }
        RequireNoOpposingOutputs(filter, prefix);
    }
}

void RunReleaseAllTest() {
    SocdState filter;
    TestEmitter emitter;

    emitter.Prepare({kSuccess});
    Require(filter.Process(Key::VerticalFirst, KeyAction::Down, emitter) == EventDisposition::Consume,
            "release all setup: vertical input was not consumed");
    Require(emitter.nextResult == emitter.attemptResults.size() &&
                filter.OutputHeld(Key::VerticalFirst),
            "release all setup: vertical output was not established");
    emitter.Prepare({kSuccess});
    Require(filter.Process(Key::HorizontalFirst, KeyAction::Down, emitter) == EventDisposition::Consume,
            "release all setup: horizontal input was not consumed");
    Require(emitter.nextResult == emitter.attemptResults.size() &&
                filter.OutputHeld(Key::HorizontalFirst),
            "release all setup: horizontal output was not established");

    emitter.Prepare({kSuccess, kSuccess});
    filter.ReleaseAll(emitter);
    const std::vector<OutputAttempt> expectedAttempts = {
        {Key::VerticalFirst, KeyAction::Up},
        {Key::HorizontalFirst, KeyAction::Up},
    };
    Require(emitter.attempts == expectedAttempts,
            "release all: expected output attempts " + DescribeAttempts(expectedAttempts) +
                ", got " + DescribeAttempts(emitter.attempts));
    Require(emitter.nextResult == emitter.attemptResults.size(),
            "release all: not all configured attempt results were used");
}

void RunAllTests() {
    constexpr Key kVerticalFirst = Key::VerticalFirst;
    constexpr Key kVerticalSecond = Key::VerticalSecond;
    constexpr Key kHorizontalFirst = Key::HorizontalFirst;
    constexpr Key kHorizontalSecond = Key::HorizontalSecond;

    RunSteps("vertical axis also uses last input priority", {
        {kVerticalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kVerticalFirst, KeyAction::Down}}, OutputFor(kVerticalFirst)},
        {kVerticalSecond, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kVerticalFirst, KeyAction::Up}, {kVerticalSecond, KeyAction::Down}},
         OutputFor(kVerticalSecond)},
    });

    RunSteps("last input priority and restoration", {
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalSecond, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}, {kHorizontalSecond, KeyAction::Down}},
         OutputFor(kHorizontalSecond)},
        {kHorizontalSecond, KeyAction::Up, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalSecond, KeyAction::Up}, {kHorizontalFirst, KeyAction::Down}},
         OutputFor(kHorizontalFirst)},
        {kHorizontalFirst, KeyAction::Up, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}}, NoOutput()},
    });

    RunSteps("last input priority in reverse order", {
        {kHorizontalSecond, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalSecond, KeyAction::Down}}, OutputFor(kHorizontalSecond)},
        {kHorizontalFirst, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalSecond, KeyAction::Up}, {kHorizontalFirst, KeyAction::Down}},
         OutputFor(kHorizontalFirst)},
        {kHorizontalFirst, KeyAction::Up, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}, {kHorizontalSecond, KeyAction::Down}},
         OutputFor(kHorizontalSecond)},
        {kHorizontalSecond, KeyAction::Up, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalSecond, KeyAction::Up}}, NoOutput()},
    });

    RunSteps("vertical and horizontal axes are independent", {
        {kVerticalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kVerticalFirst, KeyAction::Down}}, OutputFor(kVerticalFirst)},
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kVerticalFirst, kHorizontalFirst)},
        {kVerticalSecond, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kVerticalFirst, KeyAction::Up}, {kVerticalSecond, KeyAction::Down}},
         OutputFor(kVerticalSecond, kHorizontalFirst)},
        {kHorizontalSecond, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}, {kHorizontalSecond, KeyAction::Down}},
         OutputFor(kVerticalSecond, kHorizontalSecond)},
    });

    RunSteps("release failure keeps the existing direction", {
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalSecond, KeyAction::Down, {kFailure}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}}, OutputFor(kHorizontalFirst)},
    });

    RunSteps("initial press and release can fall back to the original event", {
        {kHorizontalFirst, KeyAction::Down, {kFailure}, EventDisposition::PassThrough,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalFirst, KeyAction::Up, {kFailure}, EventDisposition::PassThrough,
         {{kHorizontalFirst, KeyAction::Up}}, NoOutput()},
    });

    RunSteps("press fallback is blocked after a synthetic release", {
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalSecond, KeyAction::Down, {kSuccess, kFailure}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}, {kHorizontalSecond, KeyAction::Down}}, NoOutput()},
        {kHorizontalSecond, KeyAction::Down, {kFailure}, EventDisposition::PassThrough,
         {{kHorizontalSecond, KeyAction::Down}}, OutputFor(kHorizontalSecond)},
    });

    RunSteps("untracked key-up passes through", {
        {kHorizontalFirst, KeyAction::Up, {}, EventDisposition::PassThrough, {}, NoOutput()},
    });

    RunSteps("repeated key-down does not emit another transition", {
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalFirst, KeyAction::Down, {}, EventDisposition::Consume, {},
         OutputFor(kHorizontalFirst)},
    });

    RunSteps("repeated key-down does not change priority", {
        {kHorizontalFirst, KeyAction::Down, {kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Down}}, OutputFor(kHorizontalFirst)},
        {kHorizontalSecond, KeyAction::Down, {kSuccess, kSuccess}, EventDisposition::Consume,
         {{kHorizontalFirst, KeyAction::Up}, {kHorizontalSecond, KeyAction::Down}},
         OutputFor(kHorizontalSecond)},
        {kHorizontalFirst, KeyAction::Down, {}, EventDisposition::Consume, {},
         OutputFor(kHorizontalSecond)},
    });

    RunReleaseAllTest();
}

} // namespace

int main() {
    try {
        RunAllTests();
        std::cout << "SOCD state transition tests passed.\n";
        return EXIT_SUCCESS;
    } catch (const std::exception& error) {
        std::cerr << "SOCD state transition test failed: " << error.what() << '\n';
        return EXIT_FAILURE;
    }
}
