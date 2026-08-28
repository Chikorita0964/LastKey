#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>

namespace lastkey {

enum class Key : std::uint8_t {
    VerticalFirst,
    VerticalSecond,
    HorizontalFirst,
    HorizontalSecond,
    Count,
};

constexpr std::size_t kKeyCount = static_cast<std::size_t>(Key::Count);

enum class KeyAction { Down, Up };
enum class EventDisposition { Consume, PassThrough };

constexpr std::size_t ToIndex(Key key) {
    return static_cast<std::size_t>(key);
}

class SocdState {
public:
    template <typename Emit>
    EventDisposition Process(Key key, KeyAction action, Emit& emit) {
        KeyState& state = State(key);
        if (action == KeyAction::Up && !state.physicallyHeld && !state.outputHeld)
            return EventDisposition::PassThrough;

        if (action == KeyAction::Down && !state.physicallyHeld) {
            state.physicallyHeld = true;
            state.pressOrder = ++sequence_;
        } else if (action == KeyAction::Up) {
            state.physicallyHeld = false;
        }

        return ResolveAxis(AxisFor(key), key, action, emit);
    }

    template <typename Emit>
    void ReleaseAll(Emit& emit) {
        for (std::size_t index = 0; index < kKeyCount; ++index) {
            const Key key = static_cast<Key>(index);
            if (State(key).outputHeld) emit(key, KeyAction::Up);
        }
    }

    bool OutputHeld(Key key) const { return State(key).outputHeld; }

private:
    struct KeyState {
        bool physicallyHeld = false;
        bool outputHeld = false;
        std::uint64_t pressOrder = 0;
    };

    struct Axis { Key negative; Key positive; };

    static constexpr std::array<Axis, 2> kAxes = {{
        {Key::VerticalFirst, Key::VerticalSecond},
        {Key::HorizontalFirst, Key::HorizontalSecond},
    }};
    static constexpr std::array<std::size_t, kKeyCount> kAxisByKey = {0, 0, 1, 1};

    KeyState& State(Key key) { return states_[ToIndex(key)]; }
    const KeyState& State(Key key) const { return states_[ToIndex(key)]; }
    const Axis& AxisFor(Key key) const { return kAxes[kAxisByKey[ToIndex(key)]]; }

    std::optional<Key> WinnerFor(const Axis& axis) const {
        const KeyState& negative = State(axis.negative);
        const KeyState& positive = State(axis.positive);
        if (negative.physicallyHeld && !positive.physicallyHeld) return axis.negative;
        if (positive.physicallyHeld && !negative.physicallyHeld) return axis.positive;
        if (negative.physicallyHeld && positive.physicallyHeld)
            return negative.pressOrder > positive.pressOrder ? axis.negative : axis.positive;
        return std::nullopt;
    }

    // Reconciles emitted output with the desired direction for this axis.
    // Pass through an original event only when it cannot create opposing output.
    template <typename Emit>
    EventDisposition ResolveAxis(const Axis& axis, Key originalKey, KeyAction action, Emit& emit) {
        const std::optional<Key> desiredOutput = WinnerFor(axis);
        bool releasedPreviousOutput = false;

        for (const Key key : {axis.negative, axis.positive}) {
            if (State(key).outputHeld && (!desiredOutput || key != *desiredOutput)) {
                if (!emit(key, KeyAction::Up)) {
                    if (action == KeyAction::Up && originalKey == key) {
                        State(key).outputHeld = false;
                        return EventDisposition::PassThrough;
                    }
                    return EventDisposition::Consume;
                }
                State(key).outputHeld = false;
                releasedPreviousOutput = true;
            }
        }

        if (desiredOutput && !State(*desiredOutput).outputHeld) {
            if (emit(*desiredOutput, KeyAction::Down)) {
                State(*desiredOutput).outputHeld = true;
            } else if (action == KeyAction::Down && originalKey == *desiredOutput &&
                       !releasedPreviousOutput) {
                State(*desiredOutput).outputHeld = true;
                return EventDisposition::PassThrough;
            }
        }
        return EventDisposition::Consume;
    }

    std::array<KeyState, kKeyCount> states_{};
    std::uint64_t sequence_ = 0;
};

} // namespace lastkey
